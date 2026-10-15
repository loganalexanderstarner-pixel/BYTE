// The control strip shown at the top of BYTE's browser window when "Watch BYTE browse" is on (Settings, Browser).
// It sits in a closed shadow root, so the page can't restyle it and BYTE's own page reader never lists it.
// Stop navigates to byteagent://stop, which BYTE's navigation handler catches and cancels.
(function () {
  "use strict";
  if (window.top !== window || window.__byteStrip) return;
  window.__byteStrip = true;
  function build() {
    if (!document.documentElement || document.getElementById("byte-browsing-strip")) return;
    var host = document.createElement("div");
    host.id = "byte-browsing-strip";
    host.style.cssText = "all:initial;position:fixed;top:0;left:0;right:0;height:34px;z-index:2147483647;";
    var root = host.attachShadow({ mode: "closed" });
    var style = document.createElement("style");
    style.textContent =
      ".bar{box-sizing:border-box;height:34px;display:flex;align-items:center;gap:10px;padding:0 12px;background:#101418;color:#e8fff4;font:13px/1 system-ui,Segoe UI,sans-serif;border-bottom:2px solid #00e5a0}" +
      ".dot{width:9px;height:9px;border-radius:50%;background:#00e5a0}" +
      ".msg{flex:1;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}" +
      ".sub{opacity:.7}" +
      "button{font:inherit;cursor:pointer;border:1px solid #ff5d6c;background:#2a1519;color:#ffd9dd;border-radius:6px;padding:5px 12px}" +
      "button:hover{background:#ff5d6c;color:#101418}";
    var bar = document.createElement("div");
    bar.className = "bar";
    bar.innerHTML = '<span class="dot"></span><span class="msg">BYTE is browsing <span class="sub">· a private window, separate from your own browser</span></span>';
    var stop = document.createElement("button");
    stop.textContent = "Stop";
    stop.addEventListener("click", function () { window.location.href = "byteagent://stop"; });
    bar.appendChild(stop);
    root.appendChild(style);
    root.appendChild(bar);
    document.documentElement.appendChild(host);
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", build);
  else build();
})();
