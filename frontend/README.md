# Overtalk web interface

This folder holds Overtalk's Next.js/React interface, which runs in Tauri, and the Rust service in `src-tauri/`. The default desktop interface is the native Slint app in `../native/`; this web interface is a supported alternative that uses the same service. To build the default app, see [Building from Source](../docs/BUILDING.md).

## Features

- Real-time audio recording from both microphone and system audio
- Local transcription with Parakeet or Whisper models
- Native desktop integration using Tauri
- Speaker diarization support
- Rich text editor for note-taking
- Local processing by default; cloud AI providers only when you choose them

## Prerequisites

### For macOS:
- Node.js (v20 or later)
- Rust (latest stable)
- pnpm (v9.15.9)
- [Xcode Command Line Tools](https://developer.apple.com/download/all/?q=xcode)

### For Windows:
- Node.js (v20 or later)
- Rust (latest stable)
- pnpm (v9.15.9)
- Visual Studio Build Tools with C++ development tools
- Windows 10 or later


## Project Structure

```
/frontend
├── src/                   # Next.js frontend code
├── src-tauri/             # Rust backend for Tauri
├── public/                # Static assets
└── package.json           # Project dependencies
```

## Installation

Use `pnpm install --frozen-lockfile` to install the committed dependency set. When intentionally changing dependencies, update and commit `pnpm-lock.yaml`; frozen installation fails when it is out of sync.

Set `NEXT_TELEMETRY_DISABLED=1` in your shell to turn off the Next.js CLI's telemetry; CI sets it. Building the Rust service also needs the FFmpeg variables described in [Verified FFmpeg input](../docs/BUILDING.md#verified-ffmpeg-input).

### For macOS:

1. Install prerequisites:
   ```bash
   # Install Homebrew if not already installed
   /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
   
   # Install Node.js
   brew install node
   
   # Install Rust
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   
   # Install pnpm
   npm install -g pnpm@9.15.9
   
   # Install Xcode Command Line Tools
   xcode-select --install
   ```

2. Clone the repository and navigate to the frontend directory:
   ```bash
   git clone https://github.com/judeam/overtalk
   cd overtalk/frontend
   ```
  

3. Install dependencies:
   ```bash
   pnpm install --frozen-lockfile
   ```

### For Windows:

1. Install prerequisites:
   - Install [Node.js](https://nodejs.org/) (v20 or later)
   - Install [Rust](https://www.rust-lang.org/tools/install)
   - Install pnpm: `npm install -g pnpm@9.15.9`
   - Install [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with C++ development tools

2. Clone the repository and navigate to the frontend directory:
   ```cmd
   git clone https://github.com/judeam/overtalk
   cd overtalk/frontend
   ```

3. Install dependencies:
   ```cmd
   pnpm install --frozen-lockfile
   ```

## Running the App

The Tauri bundle expects the `llama-helper` sidecar in `src-tauri/binaries/`. The GPU scripts build it with the detected acceleration before starting Tauri ([GPU notes](../docs/GPU_ACCELERATION.md#automatic-gpu-detection)).

### For macOS and Linux:

Run the app in development mode:
```bash
./dev-gpu.sh
```

To build a production version:
```bash
./build-gpu.sh
```

`./clean_run.sh [info|debug|trace]` and `./clean_build.sh` reinstall dependencies and rebuild from scratch. They do not build the sidecar, so run `./dev-gpu.sh` once first.

### For Windows:

Run the app in development mode:
```cmd
dev-gpu.bat
```

To build a production version:
```cmd
build-gpu.bat
```

`clean_run_windows.bat` and `clean_build_windows.bat` rebuild from scratch but do not build the sidecar.

After the sidecar exists, you can also use the package scripts directly:
```bash
pnpm run tauri:dev
pnpm run tauri:build
```

## Local Transcription

Overtalk does not require a separate FastAPI service, Docker backend, or manually started whisper-server process. Local transcription is handled by the Rust service.

For build and acceleration details, see:

- [Building from Source](../docs/BUILDING.md)
- [GPU Acceleration](../docs/GPU_ACCELERATION.md)
- [Codebase map](../docs/CODEBASE-MAP.md)

## Development

### Frontend (Next.js)
- The frontend is built with Next.js and Tailwind CSS
- Source code is in the `src/` directory
- To run only the frontend: `pnpm run dev`

### Backend (Tauri)
- The Rust backend is in the `src-tauri/` directory
- Handles audio capture, file system access, transcription, storage, and native integrations
- To run only the Tauri development server: `pnpm run tauri:dev`

## Troubleshooting

### Common Issues on macOS
- If you encounter permission issues with scripts, make them executable:
  ```bash
  chmod +x clean_run.sh clean_build.sh
  ```
- For microphone access issues, ensure the app has microphone permissions in System Preferences

### Common Issues on Windows
- If you encounter build errors, ensure Visual Studio Build Tools are properly installed
- For audio capture issues, check Windows privacy settings for microphone access
- If the app fails to start, try running Command Prompt as administrator

## Contributing

See the [contributor guide](../CONTRIBUTING.md).

## License

This project is licensed under the MIT License; see [LICENSE.md](../LICENSE.md).
