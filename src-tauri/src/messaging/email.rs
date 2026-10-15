//! Email, read-only: a small IMAP client (TLS, LOGIN with an app password from the credential store) that reads one chosen
//! folder with `EXAMINE` (nothing is marked read or changed), and saves reply drafts into the Drafts folder with `APPEND`.
//! BYTE never sends mail itself (no SMTP): a draft is sent by the person from their mail app, or opened there as a
//! `mailto:` link after a yes.

use std::collections::BTreeMap;
use std::time::Duration;

use base64::Engine as _;
use futures_util::future::BoxFuture;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

use super::{Channel, Fetched, Msg, Transport, EMAIL};
use crate::error::{AppError, AppResult};

const STEP: Duration = Duration::from_secs(30);
/// Characters of a mail body fetched (the top of the message is enough to answer or summarise).
const BODY_BYTES: usize = 6000;
/// Mails taken per pass.
const PER_PASS: usize = 30;

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

/// One untagged response: its text (a literal shows as `\u{0}`) and the literals in order.
#[derive(Debug, Default, Clone)]
struct Resp {
    text: String,
    lits: Vec<Vec<u8>>,
}

struct Session {
    io: BufReader<Box<dyn Io>>,
    n: u32,
}

fn err(m: impl Into<String>) -> AppError {
    AppError::msg(m)
}

impl Session {
    async fn line(&mut self) -> AppResult<Vec<u8>> {
        let mut buf = vec![];
        let n = tokio::time::timeout(STEP, self.io.read_until(b'\n', &mut buf)).await.map_err(|_| err("The mail server didn't answer in time."))??;
        if n == 0 {
            return Err(err("The mail server closed the connection."));
        }
        Ok(buf)
    }

    /// One response: a line, plus the literals it announces (`{123}`), plus the rest of the line after each.
    async fn response(&mut self) -> AppResult<(Resp, Vec<u8>)> {
        let mut raw = self.line().await?;
        let mut r = Resp::default();
        loop {
            let trimmed = raw.strip_suffix(b"\r\n").or(raw.strip_suffix(b"\n")).unwrap_or(&raw);
            let size = (trimmed.last() == Some(&b'}')).then(|| trimmed.iter().rposition(|b| *b == b'{')).flatten().and_then(|i| std::str::from_utf8(&trimmed[i + 1..trimmed.len() - 1]).ok()?.trim_end_matches('+').parse::<usize>().ok().map(|n| (i, n)));
            match size {
                Some((i, n)) => {
                    r.text.push_str(&String::from_utf8_lossy(&trimmed[..i]));
                    r.text.push('\u{0}');
                    let mut lit = vec![0u8; n.min(2_000_000)];
                    tokio::time::timeout(STEP, self.io.read_exact(&mut lit)).await.map_err(|_| err("The mail server didn't answer in time."))??;
                    r.lits.push(lit);
                    raw = self.line().await?;
                }
                None => {
                    r.text.push_str(&String::from_utf8_lossy(trimmed));
                    return Ok((r, raw));
                }
            }
        }
    }

    /// Sends a command and collects the untagged responses up to the tagged answer.
    async fn command(&mut self, cmd: &str) -> AppResult<Vec<Resp>> {
        self.n += 1;
        let tag = format!("b{}", self.n);
        self.io.get_mut().write_all(format!("{tag} {cmd}\r\n").as_bytes()).await?;
        self.io.get_mut().flush().await?;
        self.finish(&tag).await
    }

    async fn finish(&mut self, tag: &str) -> AppResult<Vec<Resp>> {
        let mut out = vec![];
        loop {
            let (r, _) = self.response().await?;
            if let Some(rest) = r.text.strip_prefix(&format!("{tag} ")) {
                return if rest.starts_with("OK") { Ok(out) } else { Err(err(imap_reason(rest))) };
            }
            out.push(r);
        }
    }

    async fn login(&mut self, user: &str, password: &str) -> AppResult<()> {
        let (greeting, _) = self.response().await?;
        if !greeting.text.starts_with("* OK") && !greeting.text.starts_with("* PREAUTH") {
            return Err(err("That server didn't greet BYTE like a mail server. Check the server name."));
        }
        self.command(&format!("LOGIN {} {}", quote(user)?, quote(password)?)).await.map(|_| ()).map_err(|e| {
            if e.to_string().to_lowercase().contains("login") || e.to_string().to_lowercase().contains("auth") || e.to_string().to_lowercase().contains("credential") {
                err("The mail server didn't accept the user name or app password. Most providers need an app password (not your normal one) with IMAP switched on.")
            } else {
                e
            }
        })
    }
}

fn imap_reason(rest: &str) -> String {
    let t = rest.trim_start_matches("NO").trim_start_matches("BAD").trim();
    format!("The mail server said: {}", if t.is_empty() { "no" } else { t })
}

/// An IMAP quoted string (no line breaks or NULs allowed).
pub fn quote(s: &str) -> AppResult<String> {
    if s.chars().any(|c| c == '\r' || c == '\n' || c == '\0') {
        return Err(err("That value has a line break in it."));
    }
    Ok(format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")))
}

fn is_loopback(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

pub struct Mail {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    /// TLS (always, except a test server on this computer).
    pub tls: bool,
    pub drafts: String,
}

impl Mail {
    pub fn new(host: &str, user: &str, password: &str, drafts: &str) -> Mail {
        Mail { host: host.trim().to_string(), port: 993, user: user.trim().to_string(), password: password.to_string(), tls: true, drafts: if drafts.trim().is_empty() { "Drafts".into() } else { drafts.trim().to_string() } }
    }

    async fn open(&self) -> AppResult<Session> {
        crate::offline::guard()?;
        if self.host.is_empty() || self.host.contains(char::is_whitespace) {
            return Err(err("Enter the mail server's name first (for example imap.gmail.com)."));
        }
        if !self.tls && !is_loopback(&self.host) {
            return Err(err("BYTE only talks to a mail server over an encrypted connection."));
        }
        let tcp = tokio::time::timeout(STEP, tokio::net::TcpStream::connect((self.host.as_str(), self.port))).await.map_err(|_| err("The mail server didn't answer in time."))?.map_err(|e| err(format!("Couldn't reach the mail server: {e}")))?;
        let io: Box<dyn Io> = if self.tls {
            let mut roots = tokio_rustls::rustls::RootCertStore::empty();
            for c in rustls_native_certs::load_native_certs().certs {
                let _ = roots.add(c);
            }
            let cfg = tokio_rustls::rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(tokio_rustls::rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .map_err(|e| err(e.to_string()))?
                .with_root_certificates(roots)
                .with_no_client_auth();
            let name = tokio_rustls::rustls::pki_types::ServerName::try_from(self.host.clone()).map_err(|_| err("That isn't a valid server name."))?;
            let tls = tokio_rustls::TlsConnector::from(std::sync::Arc::new(cfg)).connect(name, tcp).await.map_err(|e| err(format!("The secure connection to the mail server failed: {e}")))?;
            Box::new(tls)
        } else {
            Box::new(tcp)
        };
        let mut s = Session { io: BufReader::new(io), n: 0 };
        s.login(&self.user, &self.password).await?;
        Ok(s)
    }

    /// Saves `rfc822` as a draft (flag \Draft), returns nothing else.
    pub async fn save_draft(&self, rfc822: &str) -> AppResult<()> {
        let mut s = self.open().await?;
        s.n += 1;
        let tag = format!("b{}", s.n);
        let data = rfc822.replace("\r\n", "\n").replace('\n', "\r\n");
        s.io.get_mut().write_all(format!("{tag} APPEND {} (\\Draft) {{{}}}\r\n", quote(&self.drafts)?, data.len()).as_bytes()).await?;
        s.io.get_mut().flush().await?;
        let (r, _) = s.response().await?;
        if !r.text.starts_with('+') {
            return Err(err(imap_reason(r.text.trim_start_matches(&format!("{tag} ")))));
        }
        s.io.get_mut().write_all(data.as_bytes()).await?;
        s.io.get_mut().write_all(b"\r\n").await?;
        s.io.get_mut().flush().await?;
        s.finish(&tag).await?;
        let _ = s.command("LOGOUT").await;
        Ok(())
    }
}

/// `* LIST (\HasNoChildren) "/" "Sent Items"` → "Sent Items" (None for folders that can't be opened).
pub fn folder_of(list_line: &str) -> Option<String> {
    let rest = list_line.strip_prefix("* LIST ")?;
    let close = rest.find(')')?;
    if rest[..close].to_lowercase().contains("\\noselect") {
        return None;
    }
    let after = rest[close + 1..].trim();
    let after = after.split_once(' ')?.1.trim(); // skip the separator
    let name = if let Some(q) = after.strip_prefix('"') { q.strip_suffix('"')?.replace("\\\"", "\"").replace("\\\\", "\\") } else { after.to_string() };
    (!name.is_empty()).then_some(name)
}

fn number_after(text: &str, key: &str) -> Option<u64> {
    let i = text.find(key)? + key.len();
    let digits: String = text[i..].chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

// ------------------------------------------------------------- mail text

/// The header fields wanted, unfolded: name (lower case) → value.
pub fn headers(raw: &[u8]) -> BTreeMap<String, String> {
    let text = String::from_utf8_lossy(raw);
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    let mut last: Option<String> = None;
    for line in text.lines() {
        if line.starts_with([' ', '\t']) {
            if let Some(k) = &last {
                if let Some(v) = out.get_mut(k) {
                    v.push(' ');
                    v.push_str(line.trim());
                }
            }
        } else if let Some((k, v)) = line.split_once(':') {
            let k = k.trim().to_lowercase();
            out.entry(k.clone()).or_insert_with(|| v.trim().to_string());
            last = Some(k);
        }
    }
    out
}

/// RFC 2047 words (`=?UTF-8?B?...?=`, `=?utf-8?Q?...?=`) decoded.
pub fn decode_words(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    let mut prev_word = false;
    while let Some(start) = rest.find("=?") {
        let (before, tail) = rest.split_at(start);
        let parts: Vec<&str> = tail[2..].splitn(4, '?').collect();
        let decoded = (parts.len() == 4 && parts[3].starts_with('=')).then(|| {
            let data = &parts[2];
            let bytes = if parts[1].eq_ignore_ascii_case("b") { base64::engine::general_purpose::STANDARD.decode(data).ok() } else { Some(qp_decode(&data.replace('_', " "), true)) };
            bytes.map(|b| String::from_utf8_lossy(&b).to_string())
        });
        match decoded.flatten() {
            Some(d) => {
                if !(prev_word && before.trim().is_empty()) {
                    out.push_str(before);
                }
                out.push_str(&d);
                let consumed = 2 + parts[0].len() + 1 + parts[1].len() + 1 + parts[2].len() + 2;
                rest = &tail[consumed.min(tail.len())..];
                prev_word = true;
            }
            None => {
                out.push_str(before);
                out.push_str("=?");
                rest = &tail[2..];
                prev_word = false;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Quoted-printable decoding (`header`: underscores were already turned into spaces by the caller).
pub fn qp_decode(s: &str, header: bool) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'=' {
            if i + 1 < b.len() && (b[i + 1] == b'\n' || b[i + 1] == b'\r') {
                i += if b[i + 1] == b'\r' && b.get(i + 2) == Some(&b'\n') { 3 } else { 2 };
                continue;
            }
            if i + 2 < b.len() + 0 {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(b[i]);
        i += 1;
    }
    let _ = header;
    out
}

fn param(value: &str, name: &str) -> Option<String> {
    let l = value.to_lowercase();
    let i = l.find(&format!("{name}="))? + name.len() + 1;
    let rest = &value[i..];
    Some(if let Some(q) = rest.strip_prefix('"') { q.split('"').next()?.to_string() } else { rest.split([';', ' ', '\r', '\n']).next()?.to_string() })
}

fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    let mut skip_until: Option<&str> = None;
    let lower = html.to_lowercase();
    let mut i = 0;
    let bytes = html.as_bytes();
    while i < bytes.len() {
        if let Some(end) = skip_until {
            if lower[i..].starts_with(end) {
                skip_until = None;
            }
            i += 1;
            continue;
        }
        let c = bytes[i];
        if c == b'<' {
            if lower[i..].starts_with("<style") {
                skip_until = Some("</style>");
            } else if lower[i..].starts_with("<script") {
                skip_until = Some("</script>");
            }
            in_tag = true;
            if lower[i..].starts_with("<br") || lower[i..].starts_with("</p") || lower[i..].starts_with("</div") {
                out.push('\n');
            }
        } else if c == b'>' && in_tag {
            in_tag = false;
        } else if !in_tag {
            out.push(c as char);
        }
        i += 1;
    }
    out.replace("&nbsp;", " ").replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'")
}

fn decode_body(body: &str, encoding: &str, charset: &str) -> String {
    let bytes = match encoding.trim().to_lowercase().as_str() {
        "base64" => base64::engine::general_purpose::STANDARD.decode(body.split_whitespace().collect::<String>()).unwrap_or_default(),
        "quoted-printable" => qp_decode(body, false),
        _ => body.as_bytes().to_vec(),
    };
    let _ = charset; // BYTE asks for UTF-8 text; other charsets show with replacement characters at worst.
    String::from_utf8_lossy(&bytes).to_string()
}

/// The readable text of a mail body: the text/plain part of a multipart message (or the HTML part with tags removed).
pub fn plain_text(top: &BTreeMap<String, String>, body: &[u8]) -> String {
    let body = String::from_utf8_lossy(body).to_string();
    let ctype = top.get("content-type").cloned().unwrap_or_else(|| "text/plain".into());
    let enc = top.get("content-transfer-encoding").cloned().unwrap_or_default();
    part_text(&ctype, &enc, &body)
}

fn part_text(ctype: &str, enc: &str, body: &str) -> String {
    let lc = ctype.to_lowercase();
    if let Some(boundary) = lc.contains("multipart/").then(|| param(ctype, "boundary")).flatten() {
        let marker = format!("--{boundary}");
        let (mut plain, mut html) = (None, None);
        for piece in body.split(&marker).skip(1) {
            let piece = piece.trim_start_matches(['\r', '\n']);
            if piece.starts_with("--") {
                break;
            }
            let (head, content) = piece.split_once("\r\n\r\n").or_else(|| piece.split_once("\n\n")).unwrap_or(("", piece));
            let h = headers(head.as_bytes());
            let pt = h.get("content-type").cloned().unwrap_or_else(|| "text/plain".into());
            let text = part_text(&pt, h.get("content-transfer-encoding").map(String::as_str).unwrap_or(""), content);
            if text.trim().is_empty() {
                continue;
            }
            if pt.to_lowercase().contains("text/html") {
                html.get_or_insert(text);
            } else if pt.to_lowercase().contains("text/plain") || pt.to_lowercase().contains("multipart/") {
                plain.get_or_insert(text);
            }
        }
        return plain.or(html).unwrap_or_default();
    }
    let text = decode_body(body, enc, "");
    if lc.contains("text/html") {
        strip_tags(&text)
    } else if lc.starts_with("text/") || lc.is_empty() {
        text
    } else {
        String::new()
    }
}

fn tidy(s: &str, max: usize) -> String {
    let mut out = String::new();
    let mut blank = 0;
    for line in s.lines() {
        let l = line.trim_end();
        if l.starts_with('>') {
            continue; // quoted earlier mail
        }
        if l.is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(l);
        out.push('\n');
    }
    out.trim().chars().take(max).collect()
}

/// One FETCH response as a message.
fn message_from(folder: &str, r: &Resp) -> Option<Msg> {
    let uid = number_after(&r.text, "UID ")?;
    let h = headers(r.lits.first()?);
    let body = r.lits.get(1).map(Vec::as_slice).unwrap_or(&[]);
    let from = decode_words(h.get("from").map(String::as_str).unwrap_or("someone"));
    let subject = decode_words(h.get("subject").map(String::as_str).unwrap_or("(no subject)"));
    let text = format!("Subject: {subject}\n\n{}", tidy(&plain_text(&h, body), 4000));
    Some(Msg {
        id: uid.to_string(),
        channel: folder.to_string(),
        author: from,
        text,
        ts: h.get("date").and_then(|d| chrono::DateTime::parse_from_rfc2822(d).ok()).map(|d| d.timestamp()).unwrap_or(0),
        from_bot: false,
        private: true,
        cursor_key: folder.to_string(),
        meta: h.get("message-id").cloned().unwrap_or_default(),
    })
}

impl Transport for Mail {
    fn kind(&self) -> &'static str {
        EMAIL
    }

    fn check(&self) -> BoxFuture<'_, AppResult<String>> {
        Box::pin(async move {
            let mut s = self.open().await?;
            let _ = s.command("LOGOUT").await;
            Ok(self.user.clone())
        })
    }

    fn channels(&self) -> BoxFuture<'_, AppResult<Vec<Channel>>> {
        Box::pin(async move {
            let mut s = self.open().await?;
            let list = s.command("LIST \"\" \"*\"").await?;
            let _ = s.command("LOGOUT").await;
            Ok(list.iter().filter_map(|r| folder_of(&r.text)).map(|f| Channel { id: f.clone(), name: f, group: self.user.clone() }).collect())
        })
    }

    fn recent<'a>(&'a self, channels: &'a [String], since: &'a BTreeMap<String, String>, limit: usize) -> BoxFuture<'a, AppResult<Fetched>> {
        Box::pin(async move {
            let mut out = Fetched::default();
            let mut s = self.open().await?;
            for folder in channels {
                let sel = s.command(&format!("EXAMINE {}", quote(folder)?)).await?;
                let next = sel.iter().find_map(|r| number_after(&r.text, "UIDNEXT ")).unwrap_or(1);
                let Some(last) = since.get(folder).and_then(|v| v.parse::<u64>().ok()) else {
                    // First time: only remember where the mailbox is now.
                    out.cursors.insert(folder.clone(), next.saturating_sub(1).to_string());
                    continue;
                };
                if next <= last + 1 {
                    continue;
                }
                let found = s.command(&format!("UID SEARCH UID {}:*", last + 1)).await?;
                let mut uids: Vec<u64> = found.iter().filter(|r| r.text.starts_with("* SEARCH")).flat_map(|r| r.text.split_whitespace().skip(2).filter_map(|n| n.parse().ok()).collect::<Vec<u64>>()).filter(|u| *u > last).collect();
                uids.sort_unstable();
                let take = limit.min(PER_PASS);
                let uids: Vec<u64> = uids.iter().rev().take(take).rev().copied().collect();
                let Some(&top) = uids.last() else { continue };
                let list = uids.iter().map(u64::to_string).collect::<Vec<_>>().join(",");
                let fetched = s
                    .command(&format!("UID FETCH {list} (UID BODY.PEEK[HEADER.FIELDS (FROM SUBJECT DATE MESSAGE-ID CONTENT-TYPE CONTENT-TRANSFER-ENCODING)] BODY.PEEK[TEXT]<0.{BODY_BYTES}>)"))
                    .await?;
                let mut msgs: Vec<Msg> = fetched.iter().filter_map(|r| message_from(folder, r)).collect();
                msgs.sort_by_key(|m| m.id.parse::<u64>().unwrap_or(0));
                out.cursors.insert(folder.clone(), top.to_string());
                out.msgs.extend(msgs);
            }
            let _ = s.command("LOGOUT").await;
            Ok(out)
        })
    }

    /// Mail is never posted: use `reply`, which saves a draft.
    fn post<'a>(&'a self, _channel: &'a str, _text: &'a str) -> BoxFuture<'a, AppResult<()>> {
        Box::pin(async { Err(err("BYTE doesn't send email. A reply is saved as a draft for you to send from your mail app.")) })
    }

    fn reply<'a>(&'a self, to: &'a Msg, text: &'a str) -> BoxFuture<'a, AppResult<String>> {
        Box::pin(async move {
            self.save_draft(&reply_draft(&self.user, to, text)).await?;
            Ok(format!("Saved in {}. Open your mail app to send it.", self.drafts))
        })
    }
}

/// The address inside `Name <a@b.c>`.
pub fn address_of(from: &str) -> String {
    match (from.rfind('<'), from.rfind('>')) {
        (Some(a), Some(b)) if a < b => from[a + 1..b].trim().to_string(),
        _ => from.trim().to_string(),
    }
}

fn subject_of(m: &Msg) -> String {
    let s = m.text.lines().next().and_then(|l| l.strip_prefix("Subject: ")).unwrap_or("").trim();
    if s.to_lowercase().starts_with("re:") {
        s.to_string()
    } else {
        format!("Re: {s}")
    }
}

fn encode_header(s: &str) -> String {
    if s.is_ascii() {
        s.replace(['\r', '\n'], " ")
    } else {
        format!("=?UTF-8?B?{}?=", base64::engine::general_purpose::STANDARD.encode(s.replace(['\r', '\n'], " ")))
    }
}

/// A reply as a mail message (UTF-8 plain text).
pub fn reply_draft(from_user: &str, to: &Msg, body: &str) -> String {
    let mut h = format!("From: {}\r\nTo: {}\r\nSubject: {}\r\n", from_user.replace(['\r', '\n'], ""), address_of(&to.author).replace(['\r', '\n'], ""), encode_header(&subject_of(to)));
    if !to.meta.is_empty() {
        h.push_str(&format!("In-Reply-To: {}\r\nReferences: {}\r\n", to.meta.replace(['\r', '\n'], ""), to.meta.replace(['\r', '\n'], "")));
    }
    h.push_str(&format!("Date: {}\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n", chrono::Local::now().to_rfc2822()));
    let enc = base64::engine::general_purpose::STANDARD.encode(body.replace("\r\n", "\n").replace('\n', "\r\n"));
    for chunk in enc.as_bytes().chunks(76) {
        h.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        h.push_str("\r\n");
    }
    h
}

/// A `mailto:` link for the same reply (the person's own mail app sends it).
pub fn mailto(to: &Msg, body: &str) -> String {
    crate::pcctl::mailto(&address_of(&to.author), &subject_of(to), body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[test]
    fn folders_come_from_list_lines() {
        assert_eq!(folder_of("* LIST (\\HasNoChildren) \"/\" \"INBOX\"").as_deref(), Some("INBOX"));
        assert_eq!(folder_of("* LIST (\\HasNoChildren) \"/\" \"Sent Items\"").as_deref(), Some("Sent Items"));
        assert_eq!(folder_of("* LIST (\\Noselect \\HasChildren) \"/\" \"[Gmail]\""), None);
        assert_eq!(folder_of("* LIST () \".\" Drafts").as_deref(), Some("Drafts"));
    }

    #[test]
    fn words_in_headers_are_decoded() {
        assert_eq!(decode_words("=?UTF-8?B?SGVsbG8gd29ybGQ=?="), "Hello world");
        assert_eq!(decode_words("=?utf-8?Q?Caf=C3=A9_menu?= <a@b.c>"), "Café menu <a@b.c>");
        assert_eq!(decode_words("plain"), "plain");
        assert_eq!(address_of("Sam Lee <sam@x.org>"), "sam@x.org");
    }

    #[test]
    fn bodies_are_read_as_text() {
        let h = headers(b"Content-Type: multipart/alternative; boundary=\"XX\"\r\n\r\n");
        let body = b"preamble\r\n--XX\r\nContent-Type: text/html\r\n\r\n<p>html version</p>\r\n--XX\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nCaf=C3=A9 at noon=\r\n!\r\n--XX--\r\n";
        assert_eq!(plain_text(&h, body).trim(), "Café at noon!");
        let h = headers(b"Content-Type: text/html\r\n\r\n");
        assert_eq!(plain_text(&h, b"<style>x{}</style><p>Hi &amp; bye</p>").trim(), "Hi & bye");
        let h = headers(b"Content-Transfer-Encoding: base64\r\n\r\n");
        assert_eq!(plain_text(&h, b"aGVsbG8=").trim(), "hello");
    }

    #[test]
    fn quoted_replies_are_dropped_from_the_text() {
        assert_eq!(tidy("Yes please.\n\n> old stuff\n> more\n\n\n\nThanks", 100), "Yes please.\n\nThanks");
    }

    #[test]
    fn the_draft_is_a_reply() {
        let m = Msg { id: "7".into(), channel: "INBOX".into(), author: "Sam <sam@x.org>".into(), text: "Subject: Lunch?\n\nhi".into(), ts: 0, from_bot: false, private: true, cursor_key: "INBOX".into(), meta: "<abc@x>".into() };
        let d = reply_draft("me@y.org", &m, "Sure, noon works.");
        assert!(d.contains("To: sam@x.org\r\n") && d.contains("Subject: Re: Lunch?\r\n") && d.contains("In-Reply-To: <abc@x>"));
        let b64 = d.split("\r\n\r\n").nth(1).unwrap().replace("\r\n", "");
        assert_eq!(String::from_utf8(base64::engine::general_purpose::STANDARD.decode(b64).unwrap()).unwrap().trim(), "Sure, noon works.");
        assert!(mailto(&m, "Sure").starts_with("mailto:") && mailto(&m, "Sure").contains("sam"));
    }

    #[test]
    fn login_values_are_quoted_safely() {
        assert_eq!(quote("a\"b\\c").unwrap(), "\"a\\\"b\\\\c\"");
        assert!(quote("a\r\nb").is_err());
    }

    /// A tiny scripted IMAP server on this computer: greets, accepts LOGIN, answers the commands the client sends.
    async fn server(script: fn(&str) -> Vec<String>) -> (u16, tokio::task::JoinHandle<Vec<String>>) {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        let h = tokio::spawn(async move {
            let (sock, _) = l.accept().await.unwrap();
            let (r, mut w) = sock.into_split();
            let mut r = BufReader::new(r);
            w.write_all(b"* OK ready\r\n").await.unwrap();
            let mut seen = vec![];
            loop {
                let mut line = String::new();
                if r.read_line(&mut line).await.unwrap() == 0 {
                    break;
                }
                let line = line.trim_end().to_string();
                let (tag, cmd) = line.split_once(' ').unwrap_or((&line, ""));
                seen.push(cmd.to_string());
                if cmd.starts_with("APPEND") {
                    let n: usize = cmd.rsplit('{').next().unwrap().trim_end_matches('}').parse().unwrap();
                    w.write_all(b"+ go ahead\r\n").await.unwrap();
                    let mut buf = vec![0u8; n + 2];
                    r.read_exact(&mut buf).await.unwrap();
                    seen.push(format!("DATA:{}", String::from_utf8_lossy(&buf)));
                    w.write_all(format!("{tag} OK done\r\n").as_bytes()).await.unwrap();
                    continue;
                }
                for l in script(cmd) {
                    w.write_all(l.replace("TAG", tag).as_bytes()).await.unwrap();
                }
                if cmd.starts_with("LOGOUT") {
                    break;
                }
            }
            seen
        });
        (port, h)
    }

    fn script(cmd: &str) -> Vec<String> {
        let c = cmd.to_uppercase();
        if c.starts_with("LOGIN") {
            vec!["TAG OK logged in\r\n".into()]
        } else if c.starts_with("EXAMINE") {
            vec!["* 5 EXISTS\r\n".into(), "* OK [UIDNEXT 106] next\r\n".into(), "TAG OK [READ-ONLY] done\r\n".into()]
        } else if c.starts_with("UID SEARCH") {
            vec!["* SEARCH 104 105\r\n".into(), "TAG OK done\r\n".into()]
        } else if c.starts_with("UID FETCH") {
            let hdr = "From: =?UTF-8?B?U2Ft?= <sam@x.org>\r\nSubject: Lunch?\r\nDate: Thu, 15 Oct 2026 10:00:00 +0000\r\nMessage-ID: <m1@x>\r\n\r\n";
            let body = "Are you free at noon?";
            vec![format!("* 4 FETCH (UID 104 BODY[HEADER.FIELDS (FROM SUBJECT DATE MESSAGE-ID)] {{{}}}\r\n{hdr} BODY[TEXT]<0> {{{}}}\r\n{body})\r\n", hdr.len(), body.len()), "TAG OK done\r\n".into()]
        } else if c.starts_with("LIST") {
            vec!["* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n".into(), "* LIST (\\HasNoChildren) \"/\" \"Drafts\"\r\n".into(), "TAG OK done\r\n".into()]
        } else {
            vec!["TAG OK bye\r\n".into()]
        }
    }

    fn mail(port: u16) -> Mail {
        Mail { host: "127.0.0.1".into(), port, user: "me@y.org".into(), password: "app pass".into(), tls: false, drafts: "Drafts".into() }
    }

    #[tokio::test]
    async fn reading_a_folder_is_read_only_and_parsed() {
        let (port, h) = server(script).await;
        let since = BTreeMap::from([("INBOX".to_string(), "103".to_string())]);
        let f = mail(port).recent(&["INBOX".to_string()], &since, 50).await.unwrap();
        assert_eq!(f.msgs.len(), 1);
        assert_eq!(f.msgs[0].author, "Sam <sam@x.org>");
        assert!(f.msgs[0].text.starts_with("Subject: Lunch?") && f.msgs[0].text.contains("free at noon"));
        assert_eq!(f.msgs[0].meta, "<m1@x>");
        assert_eq!(f.cursors["INBOX"], "105");
        let seen = h.await.unwrap();
        assert!(seen.iter().any(|c| c.starts_with("EXAMINE")), "{seen:?}");
        assert!(!seen.iter().any(|c| c.starts_with("SELECT") || c.contains("STORE") || c.contains("EXPUNGE") || c.contains(" BODY[")), "never changes anything: {seen:?}");
    }

    #[tokio::test]
    async fn the_first_read_only_sets_the_marker() {
        let (port, h) = server(script).await;
        let f = mail(port).recent(&["INBOX".to_string()], &BTreeMap::new(), 50).await.unwrap();
        assert!(f.msgs.is_empty());
        assert_eq!(f.cursors["INBOX"], "105");
        let _ = h.await;
    }

    #[tokio::test]
    async fn folders_and_the_login_check() {
        let (port, h) = server(script).await;
        let c = mail(port).channels().await.unwrap();
        assert_eq!(c.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["INBOX", "Drafts"]);
        let seen = h.await.unwrap();
        assert!(seen[0].starts_with("LOGIN \"me@y.org\" \"app pass\""));
        let (port, _h) = server(script).await;
        assert_eq!(mail(port).check().await.unwrap(), "me@y.org");
    }

    #[tokio::test]
    async fn a_reply_is_saved_as_a_draft_and_never_sent() {
        let (port, h) = server(script).await;
        let m = Msg { id: "104".into(), channel: "INBOX".into(), author: "Sam <sam@x.org>".into(), text: "Subject: Lunch?\n\nhi".into(), ts: 0, from_bot: false, private: true, cursor_key: "INBOX".into(), meta: String::new() };
        let said = mail(port).reply(&m, "Noon works.").await.unwrap();
        assert!(said.contains("Saved in Drafts"));
        let seen = h.await.unwrap();
        assert!(seen.iter().any(|c| c.starts_with("APPEND \"Drafts\" (\\Draft)")));
        assert!(seen.iter().any(|c| c.starts_with("DATA:") && c.contains("To: sam@x.org")));
        assert!(mail(1).post("INBOX", "x").await.is_err());
    }

    #[tokio::test]
    async fn plain_connections_are_refused_off_this_computer() {
        let m = Mail { host: "imap.example.com".into(), tls: false, ..mail(143) };
        assert!(m.check().await.unwrap_err().to_string().contains("encrypted"));
    }
}
