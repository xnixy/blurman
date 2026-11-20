# Submitting Blurman to the Microsoft Store

The Store package, `Blurman_<version>.0_x64.msix`, is built from the same exe as `blurman.exe`.
Build it on your PC: `cargo build --release`, then
`packaging/msix/build.ps1 -Exe target/release/blurman.exe -Out dist`. It is unsigned, so Windows will
not install it as it is; the Store signs it when it is published, so there is no certificate to
manage. Bump the version in `Cargo.toml` first: the Store takes only a higher version than it has.

## In Partner Center

1. **Packages:** upload the `.msix`.
2. **Store listing:**
   - Description: the opening of the README, and the feature list if you like.
   - Screenshots: `docs/app.png`, `docs/frosted.png`, `docs/finish.png`, `docs/behavior.png`,
     `docs/presets.png`, and `docs/settings.png`.
   - Privacy policy URL: `https://github.com/xnixy/blurman#privacy-policy`
   - Website: `https://github.com/xnixy/blurman`
3. **Properties:** category Personalization, or Utilities & tools.
4. **Age ratings:** the questionnaire answers are all "no": no user content, no chat, no purchases,
   and no data collection.
5. **Pricing and availability:** free.
6. **Submission options, notes for certification:** paste the notes below.

## Notes for certification

> Blurman puts frosted glass behind desktop apps the user chooses. For each chosen app it makes the
> app's window partly see-through with SetLayeredWindowAttributes and places a borderless window of
> its own, with a blurred composition backdrop, directly behind it. It never injects code into
> other processes, and every change is undone when Blurman pauses or exits.
>
> runFullTrust is needed because Blurman watches and restyles other apps' top-level windows
> (SetWinEventHook, SetWindowLongPtr, SetLayeredWindowAttributes) and registers global keyboard
> shortcuts the user can change or turn off.
>
> Two undocumented calls are used, both read-only or limited to Blurman's own windows:
> SetWindowCompositionAttribute applies the acrylic blur to Blurman's own glass windows on Windows 10,
> where the composition blur is not available; NtQueryWnfStateData reads whether Do not disturb is
> on, only if the user turns on "pause during Do not disturb".
>
> Blurman collects no data. The Store version makes no network requests at all; updates come from
> the Store.
>
> To test: open Notepad, then Blurman. Pick Notepad.exe in the list and click "Frost Notepad.exe".
> The sliders on the right change its glass as they move. Closing Blurman puts Notepad back.

## What differs in the Store version

- **Start with Windows** uses the package's startup task. Windows lists it under Settings, Apps,
  Startup, where it can also be turned off.
- **Updates** come from the Store, so the update check is not shown.
- **Rules and settings** are kept in the package's own folder. Settings, Rules and presets shows where, and
  `blurman export` and `blurman import` move them between the Store version and one you build yourself.
- **`blurman`** works in a terminal through an app execution alias.
