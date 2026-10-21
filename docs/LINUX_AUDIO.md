# Linux audio and playback

Overtalk reads the desktop's PulseAudio-compatible server, including PipeWire-Pulse. The microphone and system-audio menus use the same card and port descriptions as the desktop sound settings. Monitor sources belong only to system audio; raw ALSA hardware, surround, mixer and plug aliases do not appear.

Device labels are separate from stable routing IDs. Selecting a microphone captures its active source. Selecting an output captures that sink's monitor. Speakers and headphones sharing one sink have the same monitor signal. Ports from inactive profiles remain listed but disabled with instructions to activate them in system sound settings. Discovery and recording never change the desktop's default device, profile, volume or graph quantum.

Each recording owns a cancellable native audio connection. It requests mono float PCM at 48 kHz through the server's resampler and feeds the existing mixing, saving and transcription pipeline. Stream teardown releases the microphone on cancellation or server disconnection. Defaults are resolved afresh when a recording starts.

Linux WebKit playback uses a separate loopback-only streaming service because custom-scheme file media failed in the installed player. The native command opens only a recording belonging to the requested meeting and returns an opaque, revocable capability URL. The service has no directory browsing or arbitrary file endpoint. It supports GET/HEAD and single byte ranges, rejects foreign origins/hosts, uses bounded 64 KiB chunks and eight concurrent streams, and retains at most sixteen capabilities. Leaving a meeting revokes its capability. Recordings stay on disk; whole files are not copied into webview memory.

Build prerequisites include the PulseAudio runtime library (`libpulse0`; `libpulse-dev` is recommended for development). The AppImage bundles the media framework. The installed launcher keeps WebKit font caches separate and uses the tested NVIDIA painting settings.
