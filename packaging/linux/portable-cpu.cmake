# Included before every CMake project() in whisper.cpp/ggml (passed through
# CMAKE_PROJECT_INCLUDE_BEFORE). ggml otherwise compiles with -march=native,
# which makes a package crash on any CPU older than the build machine. With
# GGML_NATIVE off, ggml targets AVX, AVX2, FMA and F16C (x86-64-v3).
set(GGML_NATIVE OFF CACHE BOOL "Portable package build" FORCE)
