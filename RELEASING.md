# Releasing mdlive

## Cut a release

```bash
git push origin main          # push code first
./release.sh 2.1.0
```

`release.sh` does:
1. Bumps version in `Cargo.toml`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`
2. Runs `cargo generate-lockfile`
3. Commits `bump to v2.1.0`, tags `v2.1.0`, pushes to `origin`

## What happens after push

```
v2.1.0 tag pushed
  → release.yml builds 6 targets (linux/macos × gnu/musl × x86_64/aarch64)
  → tauri job builds .app + .dmg
  → publishes GitHub release w/ all artifacts
  → release:published event triggers update-homebrew.yml
  → downloads CLI binaries + DMG, computes SHAs
  → regenerates Formula/mdlive.rb + Casks/mdlive-app.rb
  → commits to bearded-giant/homebrew-tap as github-actions[bot]
```

## Watch

```bash
gh run watch -R bearded-giant/mdlive
gh run list -R bearded-giant/mdlive --workflow=update-homebrew.yml --limit 1
```

## Verify install

```bash
brew update
brew upgrade mdlive               # CLI
brew upgrade --cask mdlive-app    # GUI
```

## Secrets

| Secret | Required for |
|---|---|
| `GITHUB_TOKEN` | auto |
| `HOMEBREW_TAP_TOKEN` | update-homebrew.yml push to tap |
| `APPLE_*` (cert, password, signing identity, id, password, team id) | tauri code-signing + notarization |

Set tap token:
```bash
gh secret set HOMEBREW_TAP_TOKEN -R bearded-giant/mdlive --body "$HOMEBREW_TAP_TOKEN"
```

## Manual backfill

```bash
gh workflow run update-homebrew.yml -R bearded-giant/mdlive -f tag=v2.1.0
```

## Failure recovery

| Symptom | Fix |
|---|---|
| release.yml red on a target | fix on main, `git tag -d v2.1.0 && git push origin :v2.1.0 && ./release.sh 2.1.0` |
| Apple signing fails | `APPLE_*` secrets expired or certificate revoked. Re-export from Keychain, update secrets |
| Tap formula stale | `gh workflow run update-homebrew.yml -R bearded-giant/mdlive -f tag=v2.1.0` |
| Asset pattern mismatch | update patterns in `update-homebrew.yml` (CLI matrix + DMG name `mdlive_${VERSION}_aarch64.dmg`) |

See also: tap-wide `~/dev/homebrew-tap/RELEASING.md`.
