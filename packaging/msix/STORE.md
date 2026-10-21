# Submitting Blurman to the Microsoft Store

Each release on GitHub carries `Blurman_<version>.0_x64.msix`, built from the same exe as
`blurman.exe`. The Store signs the package when it is published, so there is no certificate to
manage. The same file can be built locally with `packaging/msix/build.ps1`.

## In Partner Center

1. **Packages:** upload the `.msix` from the release.
2. **Store listing:**
   - Description: the opening of the README, and the feature list if you like.
   - Screenshots: `docs/app.png`, `docs/finish.png`, `docs/presets.png`, and `docs/frosted.png`.
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
> To test: open Blurman, pick an open app such as Notepad in the list, and click Frost it.

## What differs in the Store version

- **Start with Windows** uses the package's startup task. Windows lists it under Settings, Apps,
  Startup, where it can also be turned off.
- **Updates** come from the Store, so the update check is not shown.
- **Rules and settings** are kept in the package's own folder. Settings, Files shows where, and
  `blurman export` and `blurman import` move them between the Store and portable versions.
- **`blurman`** works in a terminal through an app execution alias.
