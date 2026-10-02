# UI screenshots (mocked backend)

Renders the built web UI in headless Chromium with every Tauri command mocked, walks through the main screens
(onboarding, chats, projects, settings, speed/tuning, cloud chat, attachments, documents with outline approval,
cloud account, `/` prompts) and saves PNGs to `tools/ui-shots/out/` (git-ignored). Use it to *look at* UI
changes before claiming they work — this environment can't run the macOS app.

```sh
npm run build
npx vite preview --port 4173 &          # serve dist/
cd tools/ui-shots && npm i --no-save playwright@^1.63 && node shots.mjs
```

- If Chromium isn't installed, `npx playwright install chromium` (or point Playwright at an existing Chromium).
- Mock data lives in `mock()` and the `switch (cmd)` in `initScript`. Adding a command to the app? Mock it
  here too, or the screen will get `null`.
- Fake data only: fake key `byte_test_…`, `example.com` emails. Never put real keys or personal data here.
