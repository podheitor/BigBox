# Submitting BigBox to Flathub

One-time submission steps. After Flathub reviewers approve, the
`.github/workflows/flathub-update.yml` workflow keeps the published
package in sync automatically.

## 1. Validate locally

Before submitting, build the flatpak locally to make sure the manifest
works end-to-end. Requires `flatpak`, `flatpak-builder`, and the GNOME
Platform 46 runtime.

```bash
# Install build runtime + SDK extensions (one-time)
flatpak install --user flathub \
    org.gnome.Platform//46 \
    org.gnome.Sdk//46 \
    org.freedesktop.Sdk.Extension.rust-stable//23.08 \
    org.freedesktop.Sdk.Extension.node20//23.08

# Build + install into a per-user repo
flatpak-builder --user --install --force-clean build-dir \
    flatpak/io.github.podheitor.bigbox.yaml

# Run
flatpak run io.github.podheitor.bigbox
```

If the build fails because `cargo build` cannot reach crates.io,
that is expected — Flathub's build farm has network for the first
build, but a clean local sandbox does not. Either keep
`build-args: --share=network` in the manifest (Flathub allows this
for the `cargo` step but reviewers may ask you to vendor) or vendor
the dependencies first:

```bash
cd src-tauri
cargo vendor ../vendor
# Then add a vendor source entry in the manifest. See
# https://docs.flathub.org/docs/for-app-authors/build-recipe#offline-builds
```

## 2. Validate metainfo

Flathub runs `appstream-util validate-relax`. Run it locally:

```bash
appstream-util validate-relax flatpak/io.github.podheitor.bigbox.metainfo.xml
```

## 3. Submit to Flathub

1. Fork https://github.com/flathub/flathub.
2. Create a branch named `io.github.podheitor.bigbox` (the app-id) off `new-pr`:
   ```bash
   git clone git@github.com:<your-user>/flathub.git
   cd flathub
   git checkout -b io.github.podheitor.bigbox new-pr
   ```
3. Copy the three files from this repo into the Flathub fork root:
   ```bash
   cp /path/to/BigBox/flatpak/io.github.podheitor.bigbox.yaml .
   cp /path/to/BigBox/flatpak/io.github.podheitor.bigbox.desktop .
   cp /path/to/BigBox/flatpak/io.github.podheitor.bigbox.metainfo.xml .
   git add .
   git commit -m "Add io.github.podheitor.bigbox"
   git push -u origin io.github.podheitor.bigbox
   ```
4. Open a PR against `flathub/flathub:new-pr`. The PR template asks
   for a short description and verification that you own the upstream
   project (the `io.github.podheitor.*` reverse-DNS naming is the
   standard proof — Flathub allows it because the GitHub user
   `podheitor` controls the namespace).
5. The build bot runs immediately and posts results in the PR.
   Reviewer feedback usually arrives within 1–4 weeks. Address each
   comment with a force-push to the same branch.

## 4. After approval

Flathub creates the repo `flathub/io.github.podheitor.bigbox` and grants
you push access. From this point:

- **Automated updates**: configure the GitHub secret
  `FLATHUB_DEPLOY_TOKEN` (a fine-grained PAT with content +
  pull-request write access to that repo). The workflow at
  `.github/workflows/flathub-update.yml` opens a PR on every release.
- **Manual updates**: edit `tag:` and `commit:` in the manifest and
  push to `master` — Flathub's build bot picks it up automatically.

## 5. Optional: register at release-monitoring.org

If you register BigBox at https://release-monitoring.org and put the
project ID in `x-checker-data.project-id` (currently `0` placeholder),
the official `flathub-bot` opens these PRs on its own. At that point
the GitHub Action becomes redundant and can be deleted — both paths
work, the bot is just less code on your side.
