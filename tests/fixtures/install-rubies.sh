#!/usr/bin/env bash
# Builds the Rubies that tests/ruby_matrix.rs and tests/ruby_fixtures.rs compare with, through ruby-build, into
# <prefix>/<version>. Writes their colon-separated paths to <prefix>/rubies.txt. Versions that already exist are kept.
# The build dependencies must be installed first (see the ruby-matrix job in .github/workflows/ci.yml).
#
#   tests/fixtures/install-rubies.sh <prefix>

set -euo pipefail

prefix=${1:?usage: install-rubies.sh <prefix>}
mkdir -p "$prefix"

ruby_build="$prefix/ruby-build"
if [ ! -x "$ruby_build/bin/ruby-build" ]; then
  git clone --depth 1 https://github.com/rbenv/ruby-build.git "$ruby_build"
fi

# GCC 14 rejects the implicit declarations and int conversions of the 1.8.7 and 1.9.3 sources. Their OpenSSL
# extension does not build against OpenSSL 3, so they are built without it.
old_cflags="-O2 -std=gnu89 -fcommon -Wno-error=implicit-int -Wno-error=implicit-function-declaration -Wno-error=int-conversion -Wno-error=incompatible-pointer-types -Wno-error=incompatible-function-pointer-types -Wno-error=return-mismatch -Wno-error=declaration-missing-parameter-type"

build_ruby() {
  local version=$1
  shift
  if [ ! -x "$prefix/$version/bin/ruby" ]; then
    if ! env "$@" "$ruby_build/bin/ruby-build" "$version" "$prefix/$version"; then
      # ruby-build keeps the failing step's output in its own log, which the job log doesn't otherwise show.
      local log
      log=$(ls -t /tmp/ruby-build.*.log 2>/dev/null | head -n 1 || true)
      if [ -n "$log" ]; then
        echo "::group::Last 300 lines of $log"
        tail -n 300 "$log"
        echo "::endgroup::"
      fi
      return 1
    fi
  fi
}

build_ruby 1.8.7-p374 RUBY_CONFIGURE_OPTS=--with-out-ext=openssl "CFLAGS=$old_cflags"
build_ruby 1.9.3-p551 RUBY_CONFIGURE_OPTS=--with-out-ext=openssl "CFLAGS=$old_cflags"
build_ruby 2.7.8
build_ruby 3.3.8
build_ruby 4.0.7

list=""
for version in 1.8.7-p374 1.9.3-p551 2.7.8 3.3.8 4.0.7; do
  list="${list:+$list:}$prefix/$version/bin/ruby"
done
printf '%s\n' "$list" > "$prefix/rubies.txt"
echo "Rubies: $list"
