# Links whisper.cpp against the static C runtime, like sherpa-onnx's prebuilt libraries. Its
# CMakeLists.txt asks for an old CMake, where this setting would otherwise be ignored.
set(CMAKE_POLICY_DEFAULT_CMP0091 NEW)
set(CMAKE_MSVC_RUNTIME_LIBRARY MultiThreaded)
