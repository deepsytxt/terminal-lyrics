#!/bin/sh
# Installs the latest `lyrics` release into ~/.local/bin on Linux or macOS:
#
#   curl -fsSL https://raw.githubusercontent.com/mgtaco/terminal-lyrics/main/install.sh | sh
#
#   LYRICS_VERSION=0.3.0      a specific release instead of the latest
#   LYRICS_INSTALL_DIR=DIR    somewhere other than ~/.local/bin
#
# Pass them to `sh`, not `curl`: `curl ... | LYRICS_VERSION=0.3.0 sh`.
# Everything is inside `main`, so a download cut off halfway through runs
# nothing.

set -eu

repo="mgtaco/terminal-lyrics"

say() { printf '%s\n' "$*"; }
die() { printf 'install.sh: %s\n' "$*" >&2; exit 1; }

# Prints "name digest" per release asset. jq and python are not on every
# machine, so this tracks string state and depth: depth 3 is inside one asset
# object.
asset_digests() {
	awk '
		{ s = s $0 "\n" }
		END {
			depth = 0; instr = 0; esc = 0; str = ""; key = ""; want_key = 0
			n = length(s)
			for (i = 1; i <= n; i++) {
				c = substr(s, i, 1)
				if (instr) {
					if (esc) { esc = 0; str = str c }
					else if (c == "\\") esc = 1
					else if (c == "\"") {
						instr = 0
						if (depth == 3) {
							if (want_key) key = str
							else if (key == "name") name = str
							else if (key == "digest") digest = str
						}
					} else str = str c
				} else if (c == "\"") { instr = 1; str = "" }
				else if (c == "{" || c == "[") {
					depth++; want_key = (c == "{")
					if (depth == 3) { name = ""; digest = "" }
				} else if (c == "}" || c == "]") {
					if (depth == 3 && name != "") print name, digest
					depth--; want_key = 0
				} else if (c == ":") want_key = 0
				else if (c == ",") want_key = 1
			}
		}
	'
}

# A GITHUB_TOKEN, if one is set, goes to the API call only. It is there for CI:
# runners share addresses, and the unauthenticated limit is per address.
fetch() {
	auth=""
	case "$1" in
	https://api.github.com/*) [ -n "${GITHUB_TOKEN:-}" ] && auth="Authorization: Bearer $GITHUB_TOKEN" ;;
	esac
	if command -v curl >/dev/null 2>&1; then
		curl -fsSL --proto '=https' --tlsv1.2 ${auth:+-H "$auth"} -o "$2" "$1"
	elif command -v wget >/dev/null 2>&1; then
		wget -q --https-only ${auth:+--header="$auth"} -O "$2" "$1"
	else
		die "needs curl or wget to download the release"
	fi
}

sha256() {
	if command -v sha256sum >/dev/null 2>&1; then
		sha256sum "$1" | cut -d' ' -f1
	elif command -v shasum >/dev/null 2>&1; then
		shasum -a 256 "$1" | cut -d' ' -f1
	else
		die "needs sha256sum or shasum to check the download"
	fi
}

# The musl build is statically linked, so it runs however old the glibc is.
target() {
	os=$(uname -s)
	arch=$(uname -m)
	case "$os" in
	Linux)
		case "$arch" in
		x86_64 | amd64) echo x86_64-unknown-linux-musl ;;
		*) die "no release build for Linux on $arch yet; \`cargo install terminal-lyrics\` builds one" ;;
		esac
		;;
	Darwin)
		# A shell running under Rosetta reports x86_64 on Apple silicon, and
		# would install the Intel build to run translated for no reason.
		if [ "$(sysctl -n hw.optional.arm64 2>/dev/null || true)" = 1 ]; then
			arch=arm64
		fi
		case "$arch" in
		arm64 | aarch64) echo aarch64-apple-darwin ;;
		x86_64) echo x86_64-apple-darwin ;;
		*) die "no release build for macOS on $arch" ;;
		esac
		;;
	*)
		die "this script covers Linux and macOS; on $os, take the archive from https://github.com/$repo/releases/latest"
		;;
	esac
}

main() {
	target=$(target)
	dir="${LYRICS_INSTALL_DIR:-$HOME/.local/bin}"

	tmp=$(mktemp -d 2>/dev/null || mktemp -d -t lyrics)
	trap 'rm -rf "$tmp"' EXIT
	trap 'exit 1' HUP INT TERM

	if [ -n "${LYRICS_VERSION:-}" ]; then
		api="https://api.github.com/repos/$repo/releases/tags/v${LYRICS_VERSION#v}"
	else
		api="https://api.github.com/repos/$repo/releases/latest"
	fi
	fetch "$api" "$tmp/release.json" || die "could not read the release from $api"

	tag=$(sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$tmp/release.json" | head -n 1)
	[ -n "$tag" ] || die "no tag_name in the release from $api"
	version=${tag#v}
	archive="lyrics-$version-$target.tar.gz"

	digest=$(asset_digests <"$tmp/release.json" | awk -v a="$archive" '$1 == a { print $2; exit }')
	[ -n "$digest" ] || die "release $tag has no $archive"
	case "$digest" in
	sha256:*) want=${digest#sha256:} ;;
	*) die "release $tag gives no SHA-256 for $archive, so the download cannot be checked" ;;
	esac

	say "Downloading lyrics $version for $target"
	fetch "https://github.com/$repo/releases/download/$tag/$archive" "$tmp/$archive"

	got=$(sha256 "$tmp/$archive")
	[ "$got" = "$want" ] || die "$archive does not match its SHA-256 (expected $want, got $got)"

	tar -xzf "$tmp/$archive" -C "$tmp"
	# `install -D` would make the directory too, but macOS's -D is another flag.
	mkdir -p "$dir"
	install -m755 "$tmp/lyrics-$version-$target/lyrics" "$dir/lyrics"
	say "Installed lyrics $version to $dir/lyrics"

	case ":$PATH:" in
	*":$dir:"*)
		# Something earlier on PATH would still win, most likely a
		# `cargo install` copy in ~/.cargo/bin.
		found=$(command -v lyrics 2>/dev/null || true)
		if [ -n "$found" ] && [ "$found" != "$dir/lyrics" ]; then
			say ""
			say "Note: \`lyrics\` still runs $found, which comes before $dir on PATH."
		fi
		;;
	*)
		say ""
		say "$dir is not on your PATH yet. To add it:"
		case "${SHELL:-}" in
		*/fish) say "  fish_add_path $dir" ;;
		*/zsh) say "  echo 'export PATH=\"$dir:\$PATH\"' >> ~/.zshrc" ;;
		*) say "  echo 'export PATH=\"$dir:\$PATH\"' >> ~/.bashrc" ;;
		esac
		say "then open a new terminal."
		;;
	esac

	if [ "$(uname -s)" = Darwin ]; then
		say ""
		say "The first run asks to let your terminal control Spotify or Music: that is"
		say "how it reads what is playing. Allow it once and it stops asking."
	fi
}

main "$@"
