# GPU builds on Linux

This guide covers GPU-accelerated builds on Linux. The default native app is built with Cargo as described in [Build and run from source](BUILDING.md#build-and-run-from-source-linux); pass the acceleration feature as shown in [GPU notes](GPU_ACCELERATION.md#choosing-a-backend). The scripts below, `frontend/dev-gpu.sh` and `frontend/build-gpu.sh`, build the web interface and detect the GPU automatically. The Linux packages are CPU-only.

---

## Quick start

1. Install the packages in [Prerequisites](BUILDING.md#prerequisites) and set the FFmpeg variables ([Verified FFmpeg input](BUILDING.md#verified-ffmpeg-input)).
2. Install the web dependencies and run a script:

```bash
cd frontend
pnpm install --frozen-lockfile

# Development mode (with hot reload)
./dev-gpu.sh

# Production build
./build-gpu.sh
```

The scripts detect the GPU and choose the feature:

- **NVIDIA GPU with the CUDA toolkit:** CUDA acceleration.
- **AMD GPU with ROCm:** ROCm (HIPBlas) acceleration for Whisper.
- **Otherwise:** CPU.

---

## How detection works

Both scripts run `frontend/scripts/run-gpu.sh`. It:

1. Calls `frontend/scripts/auto-detect-gpu.js`, unless `TAURI_GPU_FEATURE` is set.
2. Builds `llama-helper` with the matching feature (`cuda` or `vulkan` on Linux; any other choice builds it for the CPU) and copies it to `frontend/src-tauri/binaries/llama-helper-<target triple>`.
3. For CUDA, exports `CMAKE_CUDA_STANDARD=17` and `CMAKE_POSITION_INDEPENDENT_CODE=ON`, and defaults `CMAKE_CUDA_ARCHITECTURES` to `native`.
4. Runs `pnpm run tauri:dev` or `tauri:build` (npm if pnpm is missing). These run `frontend/scripts/tauri-auto.js`, which passes `--features <feature>` to Tauri. `build-gpu.sh` also sets `NO_STRIP=true`.

### Detection priority

| Priority | Hardware        | What it checks                                                  | Result                  |
| -------- | --------------- | --------------------------------------------------------------- | ----------------------- |
| 1        | **NVIDIA CUDA** | `nvidia-smi` exists and (`CUDA_PATH` is set or `nvcc` is found)  | `--features cuda`       |
| 2        | **AMD ROCm**    | `rocm-smi` exists and (`ROCM_PATH` is set or `hipcc` is found)   | `--features hipblas`    |
| 3        | **Vulkan**      | `vulkaninfo` exists, and `VULKAN_SDK` and `BLAS_INCLUDE_DIRS` are set | `--features vulkan` |
| 4        | **OpenBLAS**    | `BLAS_INCLUDE_DIRS` is set                                       | `--features openblas`   |
| 5        | **CPU only**    | None of the above                                                | no feature              |

If `nvidia-smi` exists without the CUDA toolkit, or `rocm-smi` without ROCm, detection stops and uses the CPU.

### Common scenarios

| Your system               | Detection result | Why                                  |
| ------------------------- | ---------------- | ------------------------------------ |
| Clean Linux install       | CPU only         | No GPU toolkit found                 |
| NVIDIA GPU, drivers only  | CPU only         | CUDA toolkit not installed           |
| NVIDIA GPU + CUDA toolkit | CUDA             | Detection succeeds                   |
| AMD GPU + ROCm            | HIPBlas          | Detection succeeds                   |
| Vulkan drivers only       | CPU only         | `VULKAN_SDK` and `BLAS_INCLUDE_DIRS` not set |

GPU drivers alone are not enough. The build needs the development toolkit (CUDA toolkit, ROCm or Vulkan development files).

---

## GPU setup

### NVIDIA CUDA

Check the GPU's compute capability:

```bash
nvidia-smi --query-gpu=compute_cap --format=csv
```

Install the NVIDIA driver and a CUDA toolkit that supports that compute capability. On Ubuntu 24.04, the `nvidia-cuda-toolkit` package provides CUDA 12.0; newer GPUs may need a newer toolkit from NVIDIA. Then check:

```bash
nvidia-smi          # Shows GPU info
nvcc --version      # Shows CUDA version
```

Build:

```bash
cd frontend
./build-gpu.sh
```

`CMAKE_CUDA_ARCHITECTURES` defaults to `native`, which compiles for the detected GPU. Set it explicitly when building without the GPU present, converting the compute capability (`8.6` becomes `86`):

```bash
CMAKE_CUDA_ARCHITECTURES=86 ./build-gpu.sh
```

For mixed GCC versions and position-independent code, see [GPU builds](BUILDING.md#gpu-builds).

### Vulkan

Install the Vulkan development files (Ubuntu 24.04 package names; `vulkan-tools` provides `vulkaninfo`):

```bash
sudo apt install libvulkan-dev glslc vulkan-tools
```

Detection also requires `VULKAN_SDK` and `BLAS_INCLUDE_DIRS` to be set. To skip detection, select the feature directly:

```bash
cd frontend
TAURI_GPU_FEATURE=vulkan ./build-gpu.sh
```

### AMD ROCm

Install ROCm following [AMD's documentation](https://rocm.docs.amd.com), then:

```bash
export ROCM_PATH=/opt/rocm
rocm-smi            # Shows GPU info
hipcc --version     # Shows ROCm version

cd frontend
./build-gpu.sh
```

ROCm accelerates Whisper only; `llama-helper` is built for the CPU.

---

## Manual feature override

Set `TAURI_GPU_FEATURE` to skip detection:

```bash
TAURI_GPU_FEATURE=cuda ./dev-gpu.sh
TAURI_GPU_FEATURE=vulkan ./build-gpu.sh
TAURI_GPU_FEATURE=hipblas ./build-gpu.sh
TAURI_GPU_FEATURE=openblas ./build-gpu.sh
TAURI_GPU_FEATURE=none ./build-gpu.sh     # CPU only
```

An empty value still runs detection; use `none` for a CPU-only build.

### Build output location

After a successful build, the bundles are under the workspace target directory, for example:

```
target/release/bundle/appimage/Overtalk_<version>_amd64.AppImage
```

---

## Troubleshooting

### "NVIDIA GPU detected but CUDA not installed"

- Install a CUDA toolkit or set `CUDA_PATH`.
- `nvcc --version` should work.

### "Vulkan detected but missing dependencies"

- Set both `VULKAN_SDK` and `BLAS_INCLUDE_DIRS`, or use `TAURI_GPU_FEATURE=vulkan`.

### AppImage symbols

- `build-gpu.sh` sets `NO_STRIP=true`, so the AppImage build does not strip symbols.

### Build works but no GPU acceleration

- Look at the build output for the detection message.
- `nvidia-smi` (NVIDIA) or `rocm-smi` (AMD) should work.
- Install the development toolkit, not just drivers.

---

## Technical reference

### Feature matrix

| Mode     | Feature flag          | Detection requirements                            | Acceleration  |
| -------- | --------------------- | ------------------------------------------------- | ------------- |
| CUDA     | `--features cuda`     | `nvidia-smi` + (`CUDA_PATH` or `nvcc`)            | GPU           |
| ROCm     | `--features hipblas`  | `rocm-smi` + (`ROCM_PATH` or `hipcc`)             | GPU (Whisper) |
| Vulkan   | `--features vulkan`   | `vulkaninfo` + `VULKAN_SDK` + `BLAS_INCLUDE_DIRS` | GPU           |
| OpenBLAS | `--features openblas` | `BLAS_INCLUDE_DIRS`                               | CPU-optimized |
| CPU      | (none)                | (none)                                            | CPU only      |

### Environment variables

| Variable                          | Purpose                                         | Example            |
| --------------------------------- | ----------------------------------------------- | ------------------ |
| `TAURI_GPU_FEATURE`               | Skip detection and choose the feature           | `cuda`, `none`     |
| `CUDA_PATH`                       | CUDA installation directory                     | `/usr/local/cuda`  |
| `ROCM_PATH`                       | ROCm installation directory                     | `/opt/rocm`        |
| `VULKAN_SDK`, `BLAS_INCLUDE_DIRS` | Checked by Vulkan and OpenBLAS detection        |                    |
| `CMAKE_CUDA_ARCHITECTURES`        | GPU compute capability; defaults to `native`    | `86` (for 8.6)     |
| `CMAKE_CUDA_STANDARD`             | Set to `17` by the scripts for CUDA             |                    |
| `CMAKE_POSITION_INDEPENDENT_CODE` | Set to `ON` by the scripts for CUDA             |                    |
| `NO_STRIP`                        | Set to `true` by `build-gpu.sh`                 |                    |

---

If the build still fails, open an issue with your GPU type, distribution and the output from `./build-gpu.sh`.
