// BYTE web agent bridge. Injected into every page of the agent's hidden,
// private browser window. BYTE calls `window.__byteAgent.run(id, method, args)`
// with `eval`; the result comes back by navigating to
// `byteagent://r/<id>?d=<json>`, which BYTE's navigation handler reads and
// cancels (the page never leaves). No BYTE commands are reachable from pages.
(function () {
  "use strict";
  if (window.__byteAgent) return;

  var MAX_ELEMENTS = 150;
  var SENSITIVE_NAME = /(pass(word|wd|code)?|pwd|cvv|cvc|csc|security.?code|card.?(number|num|no)|cc.?(num|number)|iban|ssn|social.?security|routing|account.?(number|num)|\bpin\b|one.?time|otp)/i;
  var COMMIT_WORDS = /\b(submit|send|place order|order now|buy|pay|purchase|checkout|check out|confirm|sign up|register|create account|book now|reserve|subscribe|donate|delete|publish|post comment|apply now|transfer)\b/i;

  function post(id, out) {
    var payload = JSON.stringify(out === undefined ? null : out);
    if (typeof window.__byteAgentPost === "function") {
      window.__byteAgentPost(id, payload); // tests
      return;
    }
    window.location.href = "byteagent://r/" + encodeURIComponent(id) + "?d=" + encodeURIComponent(payload);
  }

  function clean(s, max) {
    s = String(s == null ? "" : s).replace(/\s+/g, " ").trim();
    return max && s.length > max ? s.slice(0, max - 1) + "…" : s;
  }

  // Labels without a trailing colon or asterisk ("Customer name:" → "Customer name").
  function tidy(s) {
    return clean(s, 80).replace(/\s*[:*]+\s*$/, "");
  }

  // True when the browser does layout (not a test DOM), so sizes mean something.
  function hasLayout() {
    var r = document.documentElement.getBoundingClientRect();
    return r.width > 0 || r.height > 0;
  }

  function isHidden(el) {
    if (el.type === "hidden") return true;
    for (var e = el; e && e.nodeType === 1; e = e.parentElement) {
      if (e.hidden || e.getAttribute("aria-hidden") === "true") return true;
      var st = window.getComputedStyle(e);
      if (st.display === "none" || st.visibility === "hidden") return true;
    }
    if (hasLayout() && el.getClientRects().length === 0) return true;
    return false;
  }

  function textOf(el) {
    return clean(el.innerText != null ? el.innerText : el.textContent);
  }

  function labelOf(el) {
    return tidy(rawLabel(el));
  }

  function rawLabel(el) {
    var a = el.getAttribute("aria-label");
    if (a) return clean(a, 80);
    var by = el.getAttribute("aria-labelledby");
    if (by) {
      var t = by.split(/\s+/).map(function (i) {
        var x = document.getElementById(i);
        return x ? textOf(x) : "";
      }).join(" ");
      if (clean(t)) return clean(t, 80);
    }
    if (el.id) {
      var lab = document.querySelector('label[for="' + (window.CSS && CSS.escape ? CSS.escape(el.id) : el.id) + '"]');
      if (lab && textOf(lab)) return clean(textOf(lab), 80);
    }
    var wrap = el.closest("label");
    if (wrap && textOf(wrap)) return clean(textOf(wrap), 80);
    var tag = el.tagName.toLowerCase();
    if (tag === "input" && /^(submit|button|reset)$/i.test(el.type) && el.value) return clean(el.value, 80);
    if (tag === "a" || tag === "button" || tag === "summary" || el.getAttribute("role")) {
      var t2 = textOf(el);
      if (t2) return clean(t2, 80);
      var img = el.querySelector("img[alt]");
      if (img && img.alt) return clean(img.alt, 80);
    }
    return clean(el.getAttribute("placeholder") || el.getAttribute("title") || el.getAttribute("name") || "", 80);
  }

  function isSensitive(el) {
    var type = (el.getAttribute("type") || "").toLowerCase();
    if (type === "password") return true;
    var ac = (el.getAttribute("autocomplete") || "").toLowerCase();
    if (/^cc-|current-password|new-password|one-time-code/.test(ac)) return true;
    var names = [el.getAttribute("name"), el.id, el.getAttribute("placeholder"), el.getAttribute("aria-label")].join(" ");
    return SENSITIVE_NAME.test(names);
  }

  function formOf(el) {
    return el.form || el.closest("form");
  }

  // Would activating this element submit a form or commit something?
  function commits(el) {
    var tag = el.tagName.toLowerCase();
    var type = (el.getAttribute("type") || "").toLowerCase();
    if (tag === "input" && (type === "submit" || type === "image")) return true;
    if (tag === "button" && formOf(el) && (type === "" || type === "submit")) return true;
    if (tag === "a") return false;
    return COMMIT_WORDS.test(labelOf(el));
  }

  function kindOf(el) {
    var tag = el.tagName.toLowerCase();
    var role = el.getAttribute("role");
    if (tag === "a") return "link";
    if (tag === "select") return "select";
    if (tag === "textarea") return "textbox";
    if (tag === "input") {
      var t = (el.getAttribute("type") || "text").toLowerCase();
      if (t === "submit" || t === "button" || t === "reset" || t === "image") return "button";
      if (t === "checkbox" || t === "radio") return t;
      return "input";
    }
    if (el.isContentEditable) return "textbox";
    if (role) return role;
    return "button";
  }

  var SELECTOR = "a[href], button, input, select, textarea, summary, [role=button], [role=link], [role=checkbox], [role=radio], [role=tab], [role=menuitem], [role=option], [contenteditable=true], [contenteditable='']";

  function describe(el, n) {
    var kind = kindOf(el);
    var d = { n: n, kind: kind, label: labelOf(el) };
    var tag = el.tagName.toLowerCase();
    if (tag === "input" || tag === "textarea") {
      d.type = (el.getAttribute("type") || (tag === "textarea" ? "textarea" : "text")).toLowerCase();
      if (kind === "checkbox" || kind === "radio") d.checked = !!el.checked;
      else if (kind !== "button") d.value = clean(el.value, 80);
    }
    if (tag === "select") {
      d.options = Array.prototype.slice.call(el.options, 0, 25).map(function (o) { return clean(o.text, 40); });
      d.value = el.selectedIndex >= 0 ? clean(el.options[el.selectedIndex].text, 40) : "";
    }
    if (tag === "a") d.href = el.href;
    if (el.disabled || el.getAttribute("aria-disabled") === "true") d.disabled = true;
    if (isSensitive(el)) d.sensitive = true;
    if (commits(el)) d.commits = true;
    if (el.hasAttribute("download")) d.download = true;
    return d;
  }

  function pageText(offset, max) {
    var root = document.querySelector("main, [role=main], article") || document.body;
    if (!root) return { text: "", total: 0 };
    var t = root.innerText != null ? root.innerText : root.textContent;
    t = String(t || "").replace(/[ \t ]+/g, " ").replace(/\s*\n\s*/g, "\n").replace(/\n{3,}/g, "\n\n").trim();
    return { text: t.slice(offset, offset + max), total: t.length };
  }

  function snapshot(args) {
    args = args || {};
    var old = document.querySelectorAll("[data-byte-n]");
    for (var i = 0; i < old.length; i++) old[i].removeAttribute("data-byte-n");
    var els = document.querySelectorAll(SELECTOR);
    var list = [];
    var more = 0;
    for (var j = 0; j < els.length; j++) {
      var el = els[j];
      if (isHidden(el)) continue;
      if (el.tagName === "A" && !labelOf(el)) continue;
      if (list.length >= MAX_ELEMENTS) { more++; continue; }
      var n = list.length + 1;
      el.setAttribute("data-byte-n", String(n));
      list.push(describe(el, n));
    }
    var offset = args.offset || 0;
    var t = pageText(offset, args.maxText || 6000);
    return {
      url: location.href,
      title: clean(document.title, 120),
      text: t.text,
      offset: offset,
      textTotal: t.total,
      elements: list,
      moreElements: more,
      forms: document.forms.length,
    };
  }

  function find(n) {
    var el = document.querySelector('[data-byte-n="' + Number(n) + '"]');
    if (!el) throw new Error("There's no element " + n + " on the page now; read the page again.");
    return el;
  }

  function setValue(el, value) {
    var proto = el.tagName === "TEXTAREA" ? window.HTMLTextAreaElement.prototype : window.HTMLInputElement.prototype;
    var desc = Object.getOwnPropertyDescriptor(proto, "value");
    if (desc && desc.set) desc.set.call(el, value); // works with React's controlled inputs
    else el.value = value;
  }

  function fire(el, type) {
    el.dispatchEvent(new Event(type, { bubbles: true }));
  }

  function formInfo(args) {
    var el = find(args.n);
    var form = formOf(el);
    var fields = [];
    if (form) {
      var items = form.querySelectorAll("input, select, textarea");
      for (var i = 0; i < items.length; i++) {
        var f = items[i];
        var type = (f.getAttribute("type") || "").toLowerCase();
        if (type === "hidden" || type === "submit" || type === "button" || type === "reset" || type === "image") continue;
        if (isHidden(f)) continue;
        var value;
        if (type === "checkbox") value = f.checked ? "yes" : "no";
        else if (type === "radio") { if (!f.checked) continue; value = clean(f.value || labelOf(f), 80); }
        else if (f.tagName === "SELECT") value = f.selectedIndex >= 0 ? clean(f.options[f.selectedIndex].text, 80) : "";
        else value = isSensitive(f) ? (f.value ? "•••• (filled in)" : "") : clean(f.value, 200);
        fields.push({ label: labelOf(f) || f.getAttribute("name") || type || "field", value: value });
      }
    }
    return {
      button: labelOf(el),
      action: form ? (form.action || location.href) : null,
      method: form ? (form.getAttribute("method") || "get").toLowerCase() : null,
      fields: fields,
    };
  }

  var methods = {
    snapshot: snapshot,
    formInfo: formInfo,
    click: function (args) {
      var el = find(args.n);
      if (el.disabled) throw new Error("That element is disabled.");
      if (commits(el) && !args.approved) return { needsApproval: true };
      var d = describe(el, args.n);
      // Links that would open a new window open here instead.
      if (el.tagName === "A" && el.getAttribute("target")) el.removeAttribute("target");
      // Click after the result is sent, so a navigation it starts isn't replaced by the reply.
      setTimeout(function () {
        try { el.scrollIntoView({ block: "center" }); } catch (e) { /* not in every DOM */ }
        el.click();
      }, 30);
      return { label: d.label, kind: d.kind, href: d.href || null };
    },
    type: function (args) {
      var el = find(args.n);
      if (isSensitive(el)) throw new Error("BYTE doesn't type into password, card or other private fields; the user has to fill that in.");
      var tag = el.tagName;
      var text = String(args.text == null ? "" : args.text);
      if (el.isContentEditable && tag !== "INPUT" && tag !== "TEXTAREA") {
        el.focus();
        el.textContent = args.append ? el.textContent + text : text;
        fire(el, "input");
        return { label: labelOf(el), value: clean(el.textContent, 80) };
      }
      if (tag !== "INPUT" && tag !== "TEXTAREA") throw new Error("Element " + args.n + " isn't a text box.");
      var type = (el.getAttribute("type") || "text").toLowerCase();
      if (/^(checkbox|radio|submit|button|reset|image|file|hidden)$/.test(type)) throw new Error("Element " + args.n + " isn't a text box; use click for it.");
      el.focus();
      setValue(el, args.append ? el.value + text : text);
      fire(el, "input");
      fire(el, "change");
      return { label: labelOf(el), value: clean(el.value, 80) };
    },
    choose: function (args) {
      var el = find(args.n);
      var want = clean(args.value).toLowerCase();
      var type = (el.getAttribute("type") || "").toLowerCase();
      if (el.tagName === "INPUT" && (type === "radio" || type === "checkbox")) {
        // A group of round buttons ("Small / Medium / Large"): pick the one named like the option.
        var group = el.name ? document.querySelectorAll('input[name="' + (window.CSS && CSS.escape ? CSS.escape(el.name) : el.name) + '"]') : [el];
        var hit = null;
        for (var g = 0; g < group.length && !hit; g++) if (labelOf(group[g]).toLowerCase() === want || String(group[g].value).toLowerCase() === want) hit = group[g];
        for (var h = 0; h < group.length && !hit; h++) if (labelOf(group[h]).toLowerCase().indexOf(want) >= 0) hit = group[h];
        if (!hit) throw new Error("No choice like \"" + args.value + "\" there.");
        if (!(type === "radio" && hit.checked)) hit.click();
        return { label: hit.name || labelOf(hit), value: labelOf(hit) || hit.value };
      }
      if (el.tagName !== "SELECT") throw new Error("Element " + args.n + " isn't a list or a set of choices; click it instead.");
      var opts = el.options;
      var pick = -1;
      for (var i = 0; i < opts.length && pick < 0; i++) if (clean(opts[i].text).toLowerCase() === want || String(opts[i].value).toLowerCase() === want) pick = i;
      for (var k = 0; k < opts.length && pick < 0; k++) if (clean(opts[k].text).toLowerCase().indexOf(want) >= 0) pick = k;
      if (pick < 0) throw new Error("No option like \"" + args.value + "\" in " + labelOf(el) + ".");
      el.selectedIndex = pick;
      fire(el, "input");
      fire(el, "change");
      return { label: labelOf(el), value: clean(opts[pick].text, 60) };
    },
    scroll: function (args) {
      var h = window.innerHeight || 800;
      var dir = args.direction || "down";
      if (dir === "top") window.scrollTo(0, 0);
      else if (dir === "bottom") window.scrollTo(0, document.documentElement.scrollHeight);
      else window.scrollBy(0, dir === "up" ? -0.8 * h : 0.8 * h);
      return { y: Math.round(window.scrollY || 0), height: document.documentElement.scrollHeight };
    },
    size: function () {
      var d = document.documentElement;
      return { width: Math.max(d.scrollWidth, d.clientWidth), height: Math.max(d.scrollHeight, d.clientHeight) };
    },
  };

  window.__byteAgent = {
    run: function (id, method, args) {
      var out;
      try {
        if (!methods[method]) throw new Error("unknown method " + method);
        out = { ok: true, value: methods[method](args || {}) };
      } catch (e) {
        out = { ok: false, error: String((e && e.message) || e) };
      }
      post(id, out);
    },
  };
})();
