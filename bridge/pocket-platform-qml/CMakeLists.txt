# Pocket.Platform QML module: thin adapters from the shell process to compositor and
# system interfaces that have no Quickshell integration:
#   - Wayland (the shell's own connection to Phoc): phosh_private keyboard-event
#     accelerator grabs (power key), wlr-output-power-management (display on/off),
#     wlr-output-management (output transform / rotation);
#   - D-Bus: logind Session.SetBrightness, iio-sensor-proxy, the squeekboard OSK
#     (sm.puri.OSK0); /proc/asound for audio presence.
# Built natively on the device (needs the device's Qt): scripts/build.sh bridge.
cmake_minimum_required(VERSION 3.21)
project(pocket-platform-qml VERSION 1.0 LANGUAGES C CXX) # C: generated wayland-*-protocol.c

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

find_package(Qt6 6.6 REQUIRED COMPONENTS Core Gui DBus Qml WaylandClient)
find_package(Qt6 6.6 REQUIRED COMPONENTS WaylandScannerTools)
qt_standard_project_setup(REQUIRES 6.6)

qt_add_qml_module(pocketplatformqml
    URI Pocket.Platform
    VERSION 1.0
    PLUGIN_TARGET pocketplatformqml
    OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/qml/Pocket/Platform
    SOURCES
        audio.h audio.cpp
        dbusservice.h dbusservice.cpp
        brightness.h brightness.cpp
        displaypower.h displaypower.cpp
        orientationsensor.h orientationsensor.cpp
        osk.h osk.cpp
        outputconfig.h outputconfig.cpp
        powerkey.h powerkey.cpp
)
qt6_generate_wayland_protocol_client_sources(pocketplatformqml
    FILES
        ${CMAKE_CURRENT_SOURCE_DIR}/protocols/phosh-private.xml
        ${CMAKE_CURRENT_SOURCE_DIR}/protocols/wlr-output-power-management-unstable-v1.xml
        ${CMAKE_CURRENT_SOURCE_DIR}/protocols/wlr-output-management-unstable-v1.xml
)
target_link_libraries(pocketplatformqml PRIVATE
    Qt6::Core Qt6::Gui Qt6::DBus Qt6::Qml Qt6::WaylandClient)
target_compile_options(pocketplatformqml PRIVATE -Wall -Wextra
    # Generated client headers refer to core interfaces (wl_surface, wl_output) without
    # including their declarations.
    "SHELL:-include wayland-client-protocol.h")

install(DIRECTORY ${CMAKE_BINARY_DIR}/qml/Pocket/Platform/ DESTINATION Pocket/Platform
        FILES_MATCHING PATTERN "qmldir" PATTERN "*.so" PATTERN "*.qmltypes")
