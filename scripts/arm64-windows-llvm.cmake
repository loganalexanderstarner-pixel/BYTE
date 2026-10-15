# Toolchain for the Windows ARM64 engines (llama.cpp and whisper.cpp), built with clang.
#
# Upstream's ggml refuses MSVC on ARM ("MSVC is not supported for ARM, use clang"), and its own
# Windows-on-Snapdragon instructions use clang for the same reason: the CPU kernels use
# intrinsics MSVC does not provide. This is clang (not clang-cl) targeting the MSVC ABI, so the
# result links against the same C runtime as everything else in the app. It cross-compiles on an
# x64 machine, which is how CI builds it; a Visual Studio developer environment for
# `x64_arm64` has to be loaded first so the ARM64 libraries are found.
set(CMAKE_SYSTEM_NAME Windows)
set(CMAKE_SYSTEM_PROCESSOR arm64)
set(target arm64-pc-windows-msvc)
set(CMAKE_C_COMPILER clang)
set(CMAKE_CXX_COMPILER clang++)
set(CMAKE_C_COMPILER_TARGET ${target})
set(CMAKE_CXX_COMPILER_TARGET ${target})

# The baseline is ARMv8.2 with dot product and half precision: every Windows ARM64 PC since the
# Snapdragon 8cx, and the Raspberry Pi 5 class of core, can run it. Upstream's own preset asks for
# armv8.7-a, which would die with an illegal instruction on any chip without i8mm (the older
# Snapdragons), the same mistake GGML_NATIVE=OFF exists to avoid on x64. The newest chips lose
# some prompt-reading speed to this and keep full generation speed, because generating is limited
# by memory, not by these instructions.
set(arch_c_flags "-march=armv8.2-a+dotprod+fp16 -fvectorize -ffp-model=fast -fno-finite-math-only")
set(warn_c_flags "-Wno-format -Wno-unused-function -Wno-unused-variable -Wno-unused-lambda-capture")
set(CMAKE_C_FLAGS_INIT "${arch_c_flags} ${warn_c_flags}")
set(CMAKE_CXX_FLAGS_INIT "${arch_c_flags} ${warn_c_flags}")
