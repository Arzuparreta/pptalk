cmake_minimum_required(VERSION 3.24)

project(pptalk-desktop VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

find_package(Qt6 6.8 REQUIRED COMPONENTS Core Gui Network Qml Quick QuickControls2 Widgets)
qt_standard_project_setup(REQUIRES 6.8)

qt_add_executable(pptalk-desktop
    src/main.cpp
    src/AppController.cpp
    src/AppController.hpp
)

qt_add_qml_module(pptalk-desktop
    URI Pptalk
    VERSION 1.0
    QML_FILES
        qml/Main.qml
        qml/components/Avatar.qml
        qml/components/ChatBubble.qml
        qml/components/DesktopAutostartSetting.qml
        qml/components/IconButton.qml
        qml/components/Onboarding.qml
)

target_link_libraries(pptalk-desktop PRIVATE
    Qt6::Core
    Qt6::Gui
    Qt6::Network
    Qt6::Qml
    Qt6::Quick
    Qt6::QuickControls2
    Qt6::Widgets
)

target_compile_definitions(pptalk-desktop PRIVATE
    QT_NO_CAST_FROM_ASCII
    QT_NO_CAST_TO_ASCII
)

install(TARGETS pptalk-desktop
    BUNDLE DESTINATION .
    RUNTIME DESTINATION bin
)

if(UNIX AND NOT APPLE)
    install(FILES ../../packaging/linux/pptalk.desktop
        DESTINATION share/applications
    )
    install(FILES ../../packaging/linux/pptalk.svg
        DESTINATION share/icons/hicolor/scalable/apps
    )
endif()
