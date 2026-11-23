# Pocket.Core QML module: a QtDBus client for org.pocketos.Core1, exposed to QML as
# `CoreClient`. Built natively on the device (needs the device's Qt): scripts/build.sh.
cmake_minimum_required(VERSION 3.21)
project(pocket-core-qml VERSION 1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

find_package(Qt6 6.6 REQUIRED COMPONENTS Core DBus Qml)
qt_standard_project_setup(REQUIRES 6.6)

qt_add_qml_module(pocketcoreqml
    URI Pocket.Core
    VERSION 1.0
    PLUGIN_TARGET pocketcoreqml
    OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/qml/Pocket/Core
    SOURCES coreclient.h coreclient.cpp appcatalog.h appcatalog.cpp
)
target_link_libraries(pocketcoreqml PRIVATE Qt6::Core Qt6::DBus Qt6::Qml)
target_compile_options(pocketcoreqml PRIVATE -Wall -Wextra)

# Installs the module directory (qmldir, plugin, qmltypes) under <prefix>/Pocket/Core.
install(DIRECTORY ${CMAKE_BINARY_DIR}/qml/Pocket/Core/ DESTINATION Pocket/Core
        FILES_MATCHING PATTERN "qmldir" PATTERN "*.so" PATTERN "*.qmltypes")

# Integration test of the bridge against a real pocketd (run inside dbus-run-session;
# POCKETD_BIN = path to the daemon). scripts/test-device.sh builds and runs it.
option(POCKET_BUILD_TESTS "Build the CoreClient integration test" OFF)
if(POCKET_BUILD_TESTS)
    find_package(Qt6 REQUIRED COMPONENTS Test)
    qt_add_executable(tst_coreclient tests/tst_coreclient.cpp coreclient.cpp coreclient.h
        appcatalog.cpp appcatalog.h)
    target_include_directories(tst_coreclient PRIVATE ${CMAKE_CURRENT_SOURCE_DIR})
    target_link_libraries(tst_coreclient PRIVATE Qt6::Core Qt6::DBus Qt6::Qml Qt6::Test)
endif()
