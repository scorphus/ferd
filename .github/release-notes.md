Download `Ferd-aarch64.zip` for Apple Silicon Macs or `Ferd-x86_64.zip` for Intel Macs, unzip it and move Ferd.app to Applications.

Ferd isn't notarized by Apple, so macOS blocks it the first time it opens. Open it once, then go to System Settings → Privacy & Security and click Open Anyway. Or run `xattr -d com.apple.quarantine /Applications/Ferd.app` before opening it.
