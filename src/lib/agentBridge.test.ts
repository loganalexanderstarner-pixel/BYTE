// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";

import bridge from "../../src-tauri/src/web_agent/bridge.js?raw";

type Reply = { ok: boolean; value?: any; error?: string };

const FORM = `
<main>
  <h1>Library card</h1>
  <p>Fill in the form to get a card.</p>
  <a href="/hours">Opening hours</a>
  <a href="/x" style="display:none">Hidden link</a>
  <form action="/apply" method="post">
    <label for="name">Full name</label><input id="name" name="name">
    <label>Email <input type="email" name="email" placeholder="you@example.com"></label>
    <input type="hidden" name="token" value="abc">
    <input type="password" name="pw" aria-label="Password">
    <input name="cc-number" autocomplete="cc-number" placeholder="Card">
    <select name="branch" aria-label="Branch"><option value="m">Main</option><option value="s">Squirrel Hill</option></select>
    <label><input type="checkbox" name="news"> Newsletter</label>
    <p>Size <label><input type="radio" name="size" value="small"> Small</label> <label><input type="radio" name="size" value="large"> Large</label></p>
    <label>Phone number: <input name="tel"></label>
    <textarea name="note" placeholder="Anything else?"></textarea>
    <button>Apply for a card</button>
  </form>
  <button type="button">Show more</button>
  <div role="button" aria-label="Place order"></div>
</main>`;

function load(html: string) {
  document.body.innerHTML = html;
  delete (window as any).__byteAgent;
  // eslint-disable-next-line no-new-func
  new Function(bridge)();
}

function call(method: string, args: object = {}): Reply {
  let got: string | null = null;
  (window as any).__byteAgentPost = (_id: string, payload: string) => (got = payload);
  (window as any).__byteAgent.run("t1", method, args);
  return JSON.parse(got!);
}

describe("web agent bridge", () => {
  beforeEach(() => load(FORM));

  it("numbers what can be used, skipping hidden things, with labels", () => {
    const r = call("snapshot");
    expect(r.ok).toBe(true);
    const els = r.value.elements;
    const labels = els.map((e: any) => e.label);
    expect(labels).toContain("Opening hours");
    expect(labels).not.toContain("Hidden link");
    expect(labels).toContain("Full name");
    expect(labels).toContain("Email");
    expect(els.every((e: any, i: number) => e.n === i + 1)).toBe(true);
    expect(els.find((e: any) => e.type === "hidden")).toBeUndefined();
    expect(document.querySelectorAll("[data-byte-n]").length).toBe(els.length);
    expect(r.value.text).toContain("Fill in the form");
    expect(r.value.forms).toBe(1);
  });

  it("marks private fields and buttons that commit", () => {
    const els = call("snapshot").value.elements;
    const by = (l: string) => els.find((e: any) => e.label === l);
    expect(by("Password").sensitive).toBe(true);
    expect(by("Card").sensitive).toBe(true);
    expect(by("Full name").sensitive).toBeUndefined();
    expect(by("Apply for a card").commits).toBe(true);
    expect(by("Show more").commits).toBeUndefined();
    expect(by("Place order").commits).toBe(true);
    expect(by("Branch").options).toEqual(["Main", "Squirrel Hill"]);
  });

  it("types with input events and refuses private fields", () => {
    const els = call("snapshot").value.elements;
    const name = els.find((e: any) => e.label === "Full name");
    const input = document.getElementById("name") as HTMLInputElement;
    const seen: string[] = [];
    input.addEventListener("input", () => seen.push("input"));
    input.addEventListener("change", () => seen.push("change"));
    const r = call("type", { n: name.n, text: "Ada Lovelace" });
    expect(r.ok).toBe(true);
    expect(input.value).toBe("Ada Lovelace");
    expect(seen).toEqual(["input", "change"]);
    const pw = els.find((e: any) => e.label === "Password");
    const bad = call("type", { n: pw.n, text: "hunter2" });
    expect(bad.ok).toBe(false);
    expect(bad.error).toMatch(/private/);
    expect((document.querySelector("[name=pw]") as HTMLInputElement).value).toBe("");
  });

  it("chooses options by text", () => {
    const els = call("snapshot").value.elements;
    const branch = els.find((e: any) => e.label === "Branch");
    const r = call("choose", { n: branch.n, value: "squirrel" });
    expect(r.value.value).toBe("Squirrel Hill");
    expect((document.querySelector("select") as HTMLSelectElement).value).toBe("s");
    expect(call("choose", { n: branch.n, value: "Mars" }).ok).toBe(false);
  });

  it("asks before submitting, lists the form, and clicks only once approved", () => {
    vi.useFakeTimers();
    const els = call("snapshot").value.elements;
    call("type", { n: els.find((e: any) => e.label === "Full name").n, text: "Ada" });
    const submit = els.find((e: any) => e.label === "Apply for a card");
    let submitted = 0;
    document.querySelector("form")!.addEventListener("submit", (e) => {
      e.preventDefault();
      submitted++;
    });
    expect(call("click", { n: submit.n }).value).toEqual({ needsApproval: true });
    vi.runAllTimers();
    expect(submitted).toBe(0);

    const info = call("formInfo", { n: submit.n }).value;
    expect(info.button).toBe("Apply for a card");
    expect(info.method).toBe("post");
    const fields = Object.fromEntries(info.fields.map((f: any) => [f.label, f.value]));
    expect(fields["Full name"]).toBe("Ada");
    expect(fields["Newsletter"]).toBe("no");
    expect(fields["Branch"]).toBe("Main");
    expect(fields["token"]).toBeUndefined();

    expect(call("click", { n: submit.n, approved: true }).ok).toBe(true);
    expect(submitted).toBe(0); // the click happens after the reply
    vi.runAllTimers();
    expect(submitted).toBe(1);
    vi.useRealTimers();
  });

  it("picks round buttons by name and drops colons from labels", () => {
    const els = call("snapshot").value.elements;
    expect(els.find((e: any) => e.label === "Phone number")).toBeDefined();
    const small = els.find((e: any) => e.label === "Small");
    const r = call("choose", { n: small.n, value: "large" });
    expect(r.ok).toBe(true);
    expect(r.value.value).toBe("Large");
    expect((document.querySelector("input[value=large]") as HTMLInputElement).checked).toBe(true);
    expect(call("choose", { n: small.n, value: "huge" }).ok).toBe(false);
  });

  it("reports elements that are gone", () => {
    call("snapshot");
    const r = call("click", { n: 999 });
    expect(r.ok).toBe(false);
    expect(r.error).toMatch(/read the page again/);
    expect(call("nope").ok).toBe(false);
  });
});
