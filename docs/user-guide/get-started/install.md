# Download and install CueRithm

<p class="lead">Choose the file for your computer, install it, and run one check that proves the bundled decoder and analysis model are intact. Allow about five minutes.</p>

CueRithm is a desktop app. There is no account, no sign-in and nothing to install besides the download itself. Every release is on the [releases page](https://github.com/johnneerdael/CueRithm/releases/latest) and the three steps are the same everywhere: download, install, open. The system-specific steps live on the pages for [Mac](platforms/macos.md), [Windows](platforms/windows.md) and [Linux](platforms/linux.md).

## Check that your computer can run CueRithm { #requirements }

| What | You need |
|---|---|
| Mac | Apple silicon (M1 or later) and macOS 11 or later. There is no Intel Mac build. |
| Windows | A 64-bit Intel or AMD PC (x64) running Windows 10 or 11 with the Microsoft Edge WebView2 Runtime. Windows 11 and up-to-date Windows 10 include it. |
| Linux | A 64-bit Intel or AMD PC (amd64) on a distribution that installs `.deb` packages and has glibc 2.39 or newer, such as Ubuntu 24.04. The package pulls in WebKitGTK 4.1 and GTK 3. |
| Audio files | MP3, AAC (ADTS and M4A), ALAC (M4A), FLAC, WAV, AIFF, Ogg Vorbis, Opus and WMA. Files protected by digital rights management cannot be decoded. See [Check that your files are supported](../files/supported-formats.md). |
| Internet | Not needed to analyse tracks, place cues, build playlists or clean up. Only the optional catalog tagging and recording identification use the network. |
| Extra software | None. The decoder ships inside the download, so you do not install FFmpeg, Python or a codec pack. |

## Choose the right download { #choose-download }

Every release lists the files below under **Assets**. The version number is part of the file name, and the file names spell the app "Cuerithm".

| Your computer | Download | Signed? | Steps |
|---|---|---|---|
| Mac with Apple silicon | `Cuerithm-arm64-<version>.dmg` | Developer ID signed and notarized by Apple | [Install on a Mac](platforms/macos.md) |
| Windows PC, x64 | `Cuerithm-windows-x64-<version>.zip` | Unsigned portable app, no installer | [Install on Windows](platforms/windows.md) |
| Linux PC, amd64 | `Cuerithm-linux-x64-<version>.deb` | Unsigned package | [Install on Linux](platforms/linux.md) |

## What is in a release { #what-is-in-a-release }

Only the first three files are for installing. The rest let you read what you are running.

| File | What it is |
|---|---|
| `Cuerithm-arm64-<version>.dmg` | The Mac app, signed and notarized. |
| `Cuerithm-windows-x64-<version>.zip` | The Windows app and its `media` folder. |
| `Cuerithm-linux-x64-<version>.deb` | The Linux package. |
| `cuerithm-source.tar.gz` | The application source that matches the build. |
| `ffmpeg-9.0.2.tar.xz` | The source of the bundled decoder, published to meet the FFmpeg licence. |
| `THIRD_PARTY_NOTICES.md` | Licences and notices for everything bundled. See also [Credits and open source](../help/about/credits.md). |

## Verify your download { #verify-download }

GitHub lists a SHA-256 digest beside each file under **Assets**. Compute the same digest on your copy and compare the two. If they differ, download the file again.

=== "macOS"

    ```sh
    shasum -a 256 ~/Downloads/Cuerithm-arm64-<version>.dmg
    ```

=== "Windows"

    ```powershell
    Get-FileHash "$HOME\Downloads\Cuerithm-windows-x64-<version>.zip" -Algorithm SHA256
    ```

=== "Linux"

    ```sh
    sha256sum ~/Downloads/Cuerithm-linux-x64-<version>.deb
    ```

The Mac download carries one more guarantee: Apple has notarized it, so macOS checks the signature for you. The Windows and Linux files are unsigned, which is why the digest is worth comparing.

## Install it { #install-steps }

The short version for each system. Each heading links to the full steps, the first-launch screens and what to do when the system blocks the app.

### Mac { #macos }

Open the `.dmg`, drag **CueRithm** onto **Applications** and open it. Full steps: [Install on a Mac](platforms/macos.md).

### Windows { #windows }

Unzip the archive to a folder you will keep, leave the `media` folder next to `cuerithm-desktop.exe`, and run the program. Windows SmartScreen may warn on the first launch. Full steps: [Install on Windows](platforms/windows.md).

### Linux { #linux }

Run `sudo apt install ./Cuerithm-linux-x64-<version>.deb` from the folder that holds the file. Full steps: [Install on Linux](platforms/linux.md).

## Open CueRithm for the first time { #first-launch }

A new library opens on **Tracks** with the message *Your library starts here* and the list of supported formats. CueRithm has also created a folder called `Cuerithm/Incoming` inside your Music folder and set it up as a watch folder, so any audio you copy there is prepared automatically.

Next, follow [Prepare your first tracks](first-tracks.md), or go straight to [Keep a folder like Beatport in your library automatically](../files/watch-folders.md) if your music already has a home.

## What ships inside the download { #bundled }

Everything the analysis needs is in the download, so nothing is fetched when you first run it.

| Part | Where it is | What it does |
|---|---|---|
| Decoder | `media` folder: `ffmpeg`, `ffprobe`, `runtime.json`, `COPYING.LGPLv2.1` | Turns every supported format into one private 44.1 kHz stream. Its hashes are sealed at build time and checked on every start. See [How are audio files decoded?](../how-it-works/decoding.md) |
| Key model | Inside the program | Detects the Camelot key. Verified on every start. |
| Tempo, energy and cue code | Inside the program | Runs offline on your computer. |

The `media` folder sits in `CueRithm.app/Contents/Resources/media` on a Mac, next to `cuerithm-desktop.exe` on Windows, and in `/usr/lib/CueRithm/media` on Linux.

## Check that the install is healthy { #check-that-the-install-is-healthy }

CueRithm can verify its own bundled parts and exit. The release pipeline runs the same check on every package.

=== "macOS"

    ```sh
    /Applications/CueRithm.app/Contents/MacOS/cuerithm-desktop --diagnostics
    ```

=== "Windows"

    The Windows program does not open a console, so ask PowerShell to wait for it and save the output. Run this from the folder that holds `cuerithm-desktop.exe`:

    ```powershell
    Start-Process -Wait -NoNewWindow -RedirectStandardOutput diagnostics.txt -RedirectStandardError diagnostics-error.txt .\cuerithm-desktop.exe -ArgumentList "--diagnostics"
    Get-Content diagnostics.txt, diagnostics-error.txt
    ```

=== "Linux"

    ```sh
    /usr/bin/cuerithm-desktop --diagnostics
    ```

On success it prints one line of JSON and the version it reports is the one you installed:

```json
{"application":"Cuerithm","version":"0.1.16","decoder":"cuerithm-swr44100-stereo-f32-native-md5-v2:<ffmpeg-sha256>:<ffprobe-sha256>","resources_verified":true,"built_in_analysis_verified":true}
```

`resources_verified` means the bundled `ffmpeg` and `ffprobe` match the hashes sealed at build time. `built_in_analysis_verified` means the key model matches its pinned hash. On failure the command prints the reason and exits with code 1. The messages you can meet are *Decoder hash mismatch*, *Probe hash mismatch*, *Bundled decoder manifest is missing*, *Bundled decoder folder is missing* and *Built-in analysis component failed verification; reinstall CueRithm*. All of them mean the same thing: the files are damaged or incomplete, so download the file again and reinstall.

!!! tip "Finding your version"
    The label at the bottom of the sidebar shows your version, for example *CUERITHM 0.1.16*. The `version` in the diagnostics line is the same number. On a Mac you can also select the app in Finder and press ++cmd+i++.

## Where CueRithm keeps your data { #where-cuerithm-keeps-its-data }

| System | Data folder |
|---|---|
| macOS | `~/Library/Application Support/com.cuerithm.app` |
| Windows | `%APPDATA%\com.cuerithm.app` |
| Linux | `~/.local/share/com.cuerithm.app` |

It holds `library.sqlite` (the library), the artwork and waveform caches, `tagging/` (the copy of a file while it is being written), `library-backups/` and the key model's installed copy. Your music is not in it. By default your audio stays where it is; the optional [Rename and sort files into folders](../rename/organize-files.md) is the only feature that renames, moves or copies it. [Where CueRithm keeps your data](../files/library/where-data-lives.md) explains each part.

!!! note "A separate profile"
    Set the environment variable `CUERITHM_DATA_DIR` to a folder and CueRithm uses that folder as its data folder, which gives you a second, empty library for experiments.

## Update to a newer version { #update }

CueRithm does not update itself. To update, quit the app, download the newer file and install it the same way as the first time; your library and settings stay in the data folder. When a new version needs a newer library format, it upgrades your library the first time it opens it, and an older CueRithm cannot reopen an upgraded library. [Back up your library](../files/library/back-up-library.md) first if you may want to go back. [What's new](whats-new.md) lists what each version changed.

## Uninstall CueRithm { #uninstall }

Delete the app: drag **CueRithm** out of **Applications** on a Mac, delete the folder you unzipped on Windows, or run `sudo apt remove cue-rithm` on Linux. To remove all CueRithm data as well, delete the data folder above. Your audio files are never inside it.

## What can go wrong { #what-can-go-wrong }

| What you see | Why | What to do |
|---|---|---|
| macOS or Windows refuses to open CueRithm | The system checks software from the internet | Follow [My computer blocks CueRithm at first launch](../help/troubleshooting/first-launch-blocked.md) |
| CueRithm does not start, or the diagnostics line reports a hash error | The download is incomplete or the `media` folder is missing or changed | Check the [digest](#verify-download), then reinstall |
| The window opens but a track shows a dash or will not play | The file is missing, unsupported, or the system output is silent | See [A track shows a dash or will not play](../help/troubleshooting/no-waveform-or-playback.md) |
| You need an Intel Mac, Windows on Arm or 32-bit build | No such build exists | See [What CueRithm does not do yet](../help/not-available-yet.md) |

## Related

- [Prepare your first tracks](first-tracks.md) is the next step after installing.
- [Find your way around the window](main-window.md) names every part of the app you opened.
- [Frequently asked questions](../help/faq.md) answers install and data questions.

<!-- verified: .github/workflows/package.yml and release.yml (artefacts, names, unsigned Windows/Linux, signed macOS); tools/release/sign-macos.sh; release v0.1.13 assets and digests via gh; downloaded 0.1.13 DMG (Info.plist, arm64 Mach-O minos 11.0, codesign, spctl "Notarized Developer ID", stapler validate), .deb (control: Package cue-rithm, Depends libwebkit2gtk-4.1-0 libgtk-3-0; data layout; ELF needs GLIBC_2.39 and libasound.so.2) and Windows zip (cuerithm-desktop.exe + media/); apps/desktop/src-tauri/src/main.rs (--diagnostics, windows_subsystem), lib.rs (diagnostics JSON, CUERITHM_DATA_DIR, app_data_dir), host.rs (first-run Cuerithm/Incoming watch folder), builtin.rs and crates/cuerithm-media/src/runtime.rs (error messages); apps/desktop/src/components/Sidebar.tsx (version label); App.tsx (empty state); legacy get-started/install.md re-checked. UNVERIFIED: Windows and Linux data-folder paths are Tauri's app_data_dir convention and were not observed on those systems; the Windows --diagnostics Start-Process command was not run on Windows; Windows 10 floor follows Tauri's WebView2 note, not a test; the GitHub web page display of asset digests was not seen (the API field was). -->
