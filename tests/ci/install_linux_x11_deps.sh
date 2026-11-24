#!/bin/sh
# Install minimal Linux X11/EWMH/AT-SPI fixture dependencies using Openbox
# instead of KWin/Qt5, reusing a runner-owned apt archive cache verified
# against freshly fetched signed apt metadata.
set -eu
cache_dir=${1:?usage: install_linux_x11_deps.sh <apt-cache-dir>}
mkdir -p "$cache_dir/partial"
sudo apt-get update -o Acquire::Retries=3 -o Acquire::Languages=none
sudo apt-get install -y --no-install-recommends \
  -o Acquire::Retries=3 \
  -o Dir::Cache::archives="$cache_dir" \
  xvfb x11-utils openbox tk \
  libx11-dev dbus-bin dbus-daemon libglib2.0-bin at-spi2-core python3-gi gir1.2-gtk-3.0
sudo rm -rf "$cache_dir/lock" "$cache_dir/partial"
sudo chown -R "$(id -u):$(id -g)" "$cache_dir"
