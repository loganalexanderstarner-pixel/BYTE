import { ExternalLink } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";

import { inTauri } from "../../lib/api";

export const CLOUD_SITE = "https://byteai.bytebylogan.xyz";

/** How to get a BYTE Cloud key, with a link to the website. */
export function CloudKeySteps() {
  const open = () =>
    inTauri
      ? void openUrl(CLOUD_SITE)
      : void window.open(CLOUD_SITE, "_blank", "noopener");
  return (
    <ol className="cloud-key-steps small">
      <li>
        Make an account on the{" "}
        <button className="linklike" onClick={open}>
          BYTE website <ExternalLink size={11} aria-hidden />
        </button>{" "}
        (it's invite only, so use your invite link).
      </li>
      <li>
        Signed in, open <b>Settings</b> and scroll to the <b>bottom</b>: your
        key is there.
      </li>
      <li>
        Copy the key and paste it below. BYTE checks it with the cloud, then
        keeps it in your Mac's Keychain, never in a file.
      </li>
    </ol>
  );
}
