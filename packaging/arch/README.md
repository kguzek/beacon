# Arch Linux package

`beacon-cli-git` builds the latest development version of `beacon-cli`.

## Install from AUR

```sh
yay -S beacon-cli-git
```

Check for development updates with `yay -Syu --devel`.

## Build locally

With `base-devel` installed, run from this directory:

```sh
makepkg -si
```

See the [CLI README](../../apps/cli/README.md) for Waybar configuration.

## Packaging changes

Regenerate metadata after editing `PKGBUILD`:

```sh
makepkg --printsrcinfo > .SRCINFO
```
