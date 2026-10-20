# Releasing

Releases are built by `.github/workflows/release.yml` on GitHub Actions. Pushing a
`v*` tag builds every package and attaches them to a **draft** GitHub release; nothing
is public until the draft is published by hand.

## Packages

| File | Built with |
| --- | --- |
| `…-windows-x86_64-setup.exe` | Inno Setup, from `packaging/windows/simple-git.iss` |
| `…-windows-x86_64-portable.exe` | the release executable, C runtime linked in |
| `…-macos-universal.dmg` | `lipo` (Apple Silicon + Intel), `packaging/macos/Info.plist`, `hdiutil`; also holds `packaging/macos/install-terminal-command.command` |
| `…-linux-x86_64.AppImage` | `appimagetool`, with `packaging/linux/*.desktop` |
| `…-linux-x86_64.tar.gz` | binary, `.desktop`, icon and `packaging/linux/install.sh` |

The icon's source is `assets/icon/simple-git.svg`. The `.ico` (Windows, embedded by
`build.rs`), the `.icns` (macOS, with the usual margin) and the 64 and 256 px PNGs
(window icon and Linux) are rendered from it by `packaging/render-icons.py`: run it
after changing the SVG.

The Windows installer keeps the same `AppId` across versions so that a new setup
upgrades the installed one: never change it.

## Steps

1. Check that `main` is green and up to date.
2. Set the version in `Cargo.toml` (and run `cargo build` so `Cargo.lock` follows).
3. In `CHANGELOG.md`, replace `Unreleased` with today's date, and write
   `docs/release-notes/<version>.md` (it becomes the top of the release page).
4. Merge these changes, then tag `main` and push the tag:
   ```sh
   git tag v0.1.0
   git push origin v0.1.0
   ```
5. When the Release workflow is green, open the draft release on GitHub, try the
   packages, then publish it.

To try the packages without releasing, run the Release workflow by hand from the
Actions tab: the files are kept as workflow artifacts. Pull requests that change the
packaging, the icon or `build.rs` run it too.

## Not done yet

- Code signing: Windows SmartScreen and macOS Gatekeeper warn on first launch. Signing
  needs a Windows code-signing certificate and an Apple Developer ID (notarization);
  both would be added to the workflow as repository secrets.
