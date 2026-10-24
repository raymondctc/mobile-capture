# Android frontend foundation

The Android UI adopts create-tauri-ui 1.2.0's shadcn and Tailwind frontend foundation from upstream commit `8eb86d894c19b6df04ff883ab28b412b1e5f23ea`. Its repository is [agmmnn/tauri-ui](https://github.com/agmmnn/tauri-ui). The reference frontend was generated separately with shadcn, preserving this app's existing native project and capture backend.

The reference used `npx shadcn@4.16.1 init --template vite --base radix --preset nova` and emitted the `radix-nova` style. `components.json` retains that generated configuration. The app copies and adapts 13 generated components and the generated `cn` export. Unused Tabs and Separator files are omitted. The combobox and input-group were generated with `npx shadcn@4.16.1 add combobox --yes`. The current official Radix combobox registry uses Base UI 1.9.0 for editable selection. Its dependency is pinned and its distributed notices are included.

The generated controls now appear in Capture, Library, Timeline, event details, Setup, Doctor, Settings, operation feedback, and launch-link notices. The app wrapper guards every tool dialog dismissal while setup is locked. The event inspector remains modeless and keeps its local reads outside capture action ownership.

The design keeps the selected achromatic ledger composition. At the user’s request it uses the official shadcn Zinc dark tokens by default, with semantic warning/success colors. The HTML, native window theme and initial WebView background start dark. Hardcoded light surfaces use the same tokens as the controls. The current taste-skill source was commit `6010f91a9935f1c167dfc500fb1a475681892e6b`. Its app-screen rules apply. The large region contains actual event evidence. No mock data enters the product.

Geist and Geist Mono 5.3.0 use local Latin variable WOFF2 files. The app makes no remote font requests. `public/licenses` contains their original SIL OFL texts, distributed frontend dependency notices, and `provenance.json` with source hashes and exact package versions. Vite copies this directory into `dist/licenses`. The existing native license inventory stays in its original locations.

The original React, Vite, TypeScript, Zod, and Tauri package ranges and lock resolutions remain unchanged. New foundation dependencies and DOM test dependencies have exact versions. The original port 1420 and native hooks remain configured.

`node scripts/test-ui.mjs` runs the existing schema, policy, freshness, and launch-link tests, followed by actual ReactDOM behavior checks in jsdom. The DOM runner imports the App component through a test-only loader that omits its bootstrap mount. Production entry validation and mounting remain unchanged. The checks drive controls through labels, roles, keyboard actions, and real form submission. They cover local-read isolation, stale event reads, inspector focus, locked dialog dismissal, archive ownership, explicit Start consent, and Settings repair.

`npm run web:build` checks frontend TypeScript and creates the production bundle. Browser layout, native WebView, and backend acceptance remain separate checks. `scripts/verify.mjs` also builds and tests native code and is outside this frontend migration unit.
