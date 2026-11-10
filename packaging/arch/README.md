# Arch Linux package

`beacon-cli-git` builds the latest Beacon source from GitHub. It installs
`/usr/bin/beacon-cli`; it does not build the web page or publisher.

## Install locally

Install Arch's `base-devel` package, then run from this directory:

```sh
makepkg -si
```

This fetches the public GitHub source, not your local checkout. Push changes to
GitHub before packaging them. Waybar is optional; see the
[CLI README](../../apps/cli/README.md) for configuration.

Run `makepkg -si` again to build and install the latest source. After publishing
to AUR, users can install with `yay -S beacon-cli-git` and check development
updates with `yay -Syu --devel`.

## Publish to AUR

Add an SSH public key to your AUR account. Check that the package name is
available, then clone its separate AUR repository:

```sh
git clone ssh://aur@aur.archlinux.org/beacon-cli-git.git
```

Copy `PKGBUILD` into that repository. From the AUR checkout, test and publish:

```sh
makepkg -si
makepkg --printsrcinfo > .SRCINFO
git add PKGBUILD .SRCINFO
git commit -m "Initial package"
git push origin HEAD:master
```

Only the recipe and metadata belong in AUR, not built packages or application
source. Regenerate `.SRCINFO` whenever recipe metadata changes. Update the AUR
recipe when packaging changes, not for every upstream commit.
