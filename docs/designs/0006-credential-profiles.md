# 0005: Credential profiles

> Note: since design 0007 the app is Rust + egui (it was Electron + TypeScript + React). Outdated references below are ~~struck through~~ and followed by the current equivalent.

Status: Fulfilled

## Problem

vettr stores a single API key or OAuth token. People with several (work and personal, say) have to replace it by hand each time they change context.

## Decision

- **Profiles:** the user can save any number of credentials, each with a user-defined name ("Work", "Personal"). Names are unique (case-insensitive) and at most 40 characters.
- **Storage:** `profiles.json` under ~~`userData`~~ the data dir holds `{ lastUsedId, profiles: [{ id, name, credential }] }`, where ~~`credential` is the `safeStorage`-encrypted value in base64 (same rule as before: refuse to save when encryption is unavailable)~~ the credential itself now lives in the OS keychain (`keyring` crate) and the file keeps only metadata; saving fails when the keychain is unavailable (design 0007). The file is re-read for every operation so concurrent instances do not overwrite each other from stale memory. The legacy single-key `apikey` file is migrated to a profile named "Default" on first load.
- **Per-instance selection:** the active profile is held in memory in each app process, never read back from disk while running. `lastUsedId` on disk only seeds the profile a new instance starts with, so switching in one instance cannot affect another that is already running. Windows of one instance share an agent and so share the profile.
- **Switching:** from Settings. It restarts the agent with the new credential (keeping the stored session, like a key change) and asks first if a turn is running. If the active profile is deleted by another instance, this instance reports no key until the user picks one.
- **Top bar:** a badge next to "vettr" shows the active profile's name (or "No profile"); clicking it opens Settings.
- **Settings UI:** a Profiles section lists profiles with Use, Edit (rename and/or replace the credential; blank credential keeps the old one) and Remove, plus an Add profile form. Credentials are never sent back to the ~~renderer~~ UI.
- **~~IPC~~ Backend API:** `getProfiles`, `addProfile`, `updateProfile`, `removeProfile`, `setActiveProfile` and `onProfilesChanged` replace `hasApiKey`/`setApiKey`/`clearApiKey`.

## Out of scope

Per-project profiles, a titlebar dropdown to switch, and command-palette switching.

## To do

- [x] Design
- [x] ~~`src/shared/profiles.ts`~~ `src/profiles.rs` (types, name validation)
- [x] ~~`src/main/profileStore.ts`~~ `src/host/profile_store.rs` with migration (since dropped), and tests
- [x] ~~IPC, main wiring, preload~~ `Backend` methods and wiring
- [x] Settings UI, top bar badge, first-run form
- [x] Update brief and index, mark Fulfilled
