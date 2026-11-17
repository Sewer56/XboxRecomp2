cmake_minimum_required(VERSION 3.16)
project(xbox-recomp2-xbox-cpp VERSION 0.1.0)

# Corrosion knows how to build Rust projects
include(FetchContent)
FetchContent_Declare(
  Corrosion
  GIT_REPOSITORY https://github.com/corrosion-rs/corrosion.git
  GIT_TAG v0.4)
FetchContent_MakeAvailable(Corrosion)

# Imports the crate defined by Cargo.toml (sits next to this CMakeLists file)
corrosion_import_crate(MANIFEST_PATH Cargo.toml CRATES xbox-recomp2-xbox-cpp)

add_library(xbox-recomp2-xbox STATIC src/lib.cpp)
target_include_directories(
  xbox-recomp2-xbox PUBLIC $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
                           $<INSTALL_INTERFACE:include>)

# PUBLIC so anything linking the C++ wrapper also gets the Rust staticlib
target_link_libraries(xbox-recomp2-xbox PUBLIC xbox_recomp2_xbox_cpp)
target_compile_features(xbox-recomp2-xbox PUBLIC cxx_std_20)
set_property(TARGET xbox-recomp2-xbox PROPERTY POSITION_INDEPENDENT_CODE ON)
