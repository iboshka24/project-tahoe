// SPDX-License-Identifier: GPL-3.0-or-later
// Slideshow shown while the installer copies the system. Kept to two slides
// stating facts rather than marketing: the second one is the honest status.
import QtQuick 2.0
import QtQuick.Controls 2.0

Rectangle {
    anchors.fill: parent
    color: "#1C1C22"

    Column {
        anchors.centerIn: parent
        spacing: 18
        width: parent.width * 0.8

        Text {
            anchors.horizontalCenter: parent.horizontalCenter
            color: "#F5F5F7"
            font.pixelSize: 30
            font.bold: true
            text: qsTr("Installing Project Tahoe")
        }
        Text {
            width: parent.width
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
            color: "#A0A0AC"
            font.pixelSize: 16
            text: qsTr("An Arch-based system with a macOS-styled Wayland desktop, and one execution model for Linux, Windows, macOS and AppImage programs.")
        }
        Rectangle {
            anchors.horizontalCenter: parent.horizontalCenter
            width: parent.width
            height: 1
            color: "#3A3A44"
        }
        Text {
            width: parent.width
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
            color: "#A0A0AC"
            font.pixelSize: 14
            text: qsTr("Windows and macOS programs run through Wine, Proton or Darling in separate processes; Tahoe never modifies them.")
        }
    }
}
