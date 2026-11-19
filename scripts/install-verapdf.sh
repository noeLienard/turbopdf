#!/usr/bin/env bash
# Install the veraPDF command line (PDF/A and PDF/UA validator, needs Java 11+) into $1, for CI:
#   scripts/install-verapdf.sh /opt/verapdf  →  /opt/verapdf/verapdf
set -euo pipefail
dest="${1:?usage: install-verapdf.sh <dir>}"
version=1.30.4
sha256=04415ef3f149526a3048b9889723e81320ddf013bd891bcc7f0f297f3eadcc23
work="$(mktemp -d)"
curl -sfL -o "$work/verapdf.zip" "https://software.verapdf.org/rel/${version%.*}/verapdf-greenfield-$version-installer.zip"
echo "$sha256  $work/verapdf.zip" | shasum -a 256 -c -
unzip -q "$work/verapdf.zip" -d "$work"
cat > "$work/auto-install.xml" <<EOF
<?xml version="1.0" encoding="UTF-8" standalone="no"?>
<AutomatedInstallation langpack="eng">
  <com.izforge.izpack.panels.htmlhello.HTMLHelloPanel id="welcome"/>
  <com.izforge.izpack.panels.target.TargetPanel id="install_dir"><installpath>$dest</installpath></com.izforge.izpack.panels.target.TargetPanel>
  <com.izforge.izpack.panels.packs.PacksPanel id="sdk_pack_select">
    <pack index="0" name="veraPDF GUI" selected="true"/>
    <pack index="1" name="veraPDF Batch files" selected="true"/>
    <pack index="2" name="veraPDF Validation model" selected="false"/>
    <pack index="3" name="veraPDF Documentation" selected="false"/>
    <pack index="4" name="veraPDF Sample Plugins" selected="false"/>
  </com.izforge.izpack.panels.packs.PacksPanel>
  <com.izforge.izpack.panels.install.InstallPanel id="install"/>
  <com.izforge.izpack.panels.finish.FinishPanel id="finish"/>
</AutomatedInstallation>
EOF
java -jar "$work/verapdf-greenfield-$version/verapdf-izpack-installer-$version.jar" "$work/auto-install.xml"
"$dest/verapdf" --version
