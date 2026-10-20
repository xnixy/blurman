# Signing releases with SignPath

Releases are signed by the [SignPath Foundation](https://signpath.org), which gives open source
projects a free code signing certificate. The release workflow already does the signing. These
are the one-time steps that only the project owner can take.

1. **Turn on two-factor authentication** for the GitHub account, if it is not on already. SignPath
   requires it for everyone on the team, on GitHub and on SignPath.
2. **Apply** at <https://signpath.org/apply>. Useful answers:
   - Repository: <https://github.com/xnixy/blurman>, MIT license.
   - Build: GitHub Actions on GitHub-hosted runners, `.github/workflows/release.yml`.
   - Code signing policy: <https://github.com/xnixy/blurman#code-signing-policy>.
   - Network use: none, unless the user turns on the update check or clicks Check now.
3. **Once accepted**, in SignPath:
   - Add the predefined **GitHub.com** trusted build system and link it to the project, and
     install the SignPath GitHub App on the repository.
   - Create the project with the slug `blurman` and a signing policy with the slug
     `release-signing`, or use other slugs and set them as the variables below.
   - Paste `artifact-configuration.xml` from this folder as the project's artifact configuration.
     If SignPath rejects the `${version}` parameter in `product-version`, replace it with the
     version pattern SignPath suggests.
   - Make a CI user with submitter permission and create an API token for it.
4. **In GitHub**, under Settings, Secrets and variables, Actions:
   - Secret `SIGNPATH_API_TOKEN`: the API token.
   - Variable `SIGNPATH_ORGANIZATION_ID`: the SignPath organization ID.
   - Variables `SIGNPATH_PROJECT_SLUG` and `SIGNPATH_POLICY_SLUG`, only if they are not
     `blurman` and `release-signing`.

After that, bumping the version in `Cargo.toml` and pushing to `main` builds the release and
sends it to SignPath. Approve the signing request in SignPath within a few hours. The workflow
waits for it, then publishes the signed exe and its winget manifest.
