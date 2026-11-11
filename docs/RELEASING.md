# Preparing a release

The first releases should be marked **prerelease**. Check VALIDATION.txt before
making compatibility claims. Build artifacts from reviewed source; do not upload
an installed app bundle, personal configuration, state files, logs, or backups.

1. Run formatting, tests, Clippy and native release builds on Linux and Mac.
2. Run the optional Linux audio fixture and a real pointer handoff in both
   directions on the release hardware. Check return, mute, disconnect, and default
   restoration. Record OS/architecture coverage without identifying the machines.
3. Review dependency advisories and licenses for Cargo.lock, including Mac-only
   dependencies. Binary redistribution must carry applicable dependency notices.
4. Run the privacy scanner and inspect `git diff --cached` and the archive list.
5. Build packages with `scripts/package-mac.sh` or `scripts/package-linux.sh`.
   Mac ZIPs contain an app bundle, not private installed state. Linux archives
   contain a prebuilt executable and user-level install/uninstall scripts.
6. Verify the Mac bundle signature, test first-launch setup in a fresh account,
   and verify archive checksums. Ad-hoc signing is not Developer ID notarization.
   For a smoother public Mac download, use an existing unlocked signing identity
   and notarization profile. The packager accepts `LAN_MIC_CODESIGN_IDENTITY` and
   `LAN_MIC_NOTARY_PROFILE`; it signs with hardened runtime/audio-input entitlement,
   submits, staples, and verifies. Store credentials only in the normal keychain or
   CI secret store; never commit their values. The installer preserves the complete
   prebuilt app signature/ticket. Source-binary installs are ad-hoc signed and can
   require microphone permission renewal after rebuilding.
7. Tag the tested commit `vX.Y.Z`. The release workflow builds both Mac CPU types
   and Linux x86_64, and assembles a **draft prerelease** with checksums and the
   matching source archive. Review it before publishing. Never put signing keys
   or tokens in this repository.

The GitHub repository URL is public project metadata. The author name is the
requested public credit. No personal email, development hostname, network address,
or home-directory path belongs in the source or compiled release artifacts.

The star prompt is local and one-time: after three completed forwarding sessions
and ten minutes of forwarding, it can appear while Settings is visible and the
app is foregrounded and idle. Dismissal persists across restarts. About always has
a manual GitHub link; no account checks or telemetry are used.
