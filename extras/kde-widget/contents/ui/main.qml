/*
    Виджет панели KDE для плеера «Яндекс Музыка RS».

    Данные и управление идут через MPRIS — стандартный интерфейс медиаплееров
    (org.mpris.MediaPlayer2.Player) в сессионной шине D-Bus. Плеер публикует его сам,
    поэтому виджет ничего не знает о внутренностях приложения: имя трека и исполнителя
    берёт из свойства Metadata, состояние — из PlaybackStatus, команды отправляет
    методами PlayPause, Next, Previous.

    SPDX-License-Identifier: GPL-3.0-or-later
*/

import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as QQC2

import org.kde.plasma.plasmoid
import org.kde.plasma.core as PlasmaCore
import org.kde.plasma.workspace.dbus as DBus
import org.kde.kirigami as Kirigami

PlasmoidItem {
    id: root

    readonly property string playerService: "org.mpris.MediaPlayer2.yamusic-rs"
    readonly property string playerPath: "/org/mpris/MediaPlayer2"
    readonly property string playerIface: "org.mpris.MediaPlayer2.Player"

    readonly property var metadata: props.properties["Metadata"] || ({})
    readonly property string trackTitle: metadata["xesam:title"] || ""
    readonly property string trackArtist: {
        const a = metadata["xesam:artist"];
        if (!a) {
            return "";
        }
        if (Array.isArray(a)) {
            return a.length > 0 ? String(a[0]) : "";
        }
        return String(a);
    }
    readonly property bool playing: props.properties["PlaybackStatus"] === "Playing"
    readonly property bool hasTrack: trackTitle !== "" || trackArtist !== ""
    readonly property string line: {
        if (!hasTrack) {
            return "Яндекс Музыка";
        }
        if (trackArtist === "") {
            return trackTitle;
        }
        if (trackTitle === "") {
            return trackArtist;
        }
        return trackArtist + " — " + trackTitle;
    }

    // Свойства интерфейса плеера. Значения обновляются сами по сигналу
    // PropertiesChanged, который рассылает плеер.
    DBus.Properties {
        id: props
        busType: DBus.BusType.Session
        service: root.playerService
        path: root.playerPath
        iface: root.playerIface
    }

    // Если плеер запустили позже виджета, первичное чтение свойств уже прошло —
    // раз в пять секунд перечитываем. Когда плеер закрыт, вызовы просто не проходят,
    // на вид это не влияет.
    Timer {
        interval: 5000
        running: true
        repeat: true
        onTriggered: props.updateAll()
    }

    function send(member) {
        DBus.SessionBus.asyncCall({
            "service": root.playerService,
            "path": root.playerPath,
            "iface": root.playerIface,
            "member": member,
            "signature": "",
            "arguments": []
        }, function () {
        }, function (err) {
            console.warn("yamusic-rs: команда " + member + " не прошла: " + (err && err.message ? err.message : err));
        });
    }

    compactRepresentation: RowLayout {
        spacing: Kirigami.Units.smallSpacing

        QQC2.ToolButton {
            icon.name: root.playing ? "media-playback-pause" : "media-playback-start"
            flat: true
            Layout.alignment: Qt.AlignVCenter
            onClicked: root.send("PlayPause")
            QQC2.ToolTip.visible: hovered
            QQC2.ToolTip.text: root.playing ? "Пауза" : "Играть"
        }

        QQC2.Label {
            text: root.line
            color: Kirigami.Theme.textColor
            font: Kirigami.Theme.defaultFont
            elide: Text.ElideRight
            Layout.maximumWidth: Kirigami.Units.gridUnit * 14
            Layout.alignment: Qt.AlignVCenter
        }
    }

    fullRepresentation: ColumnLayout {
        Layout.minimumWidth: Kirigami.Units.gridUnit * 16
        spacing: Kirigami.Units.smallSpacing

        QQC2.Label {
            text: root.trackArtist
            visible: text !== ""
            color: Kirigami.Theme.textColor
            font: Kirigami.Theme.defaultFont
            elide: Text.ElideRight
            Layout.fillWidth: true
            horizontalAlignment: Text.AlignHCenter
        }

        QQC2.Label {
            text: root.hasTrack ? root.trackTitle : "Плеер ничего не играет"
            color: Kirigami.Theme.textColor
            font.family: Kirigami.Theme.defaultFont.family
            font.pointSize: Kirigami.Theme.defaultFont.pointSize
            font.bold: true
            elide: Text.ElideRight
            Layout.fillWidth: true
            horizontalAlignment: Text.AlignHCenter
        }

        RowLayout {
            Layout.alignment: Qt.AlignHCenter
            spacing: Kirigami.Units.largeSpacing

            QQC2.ToolButton {
                icon.name: "media-skip-backward"
                flat: true
                onClicked: root.send("Previous")
                QQC2.ToolTip.visible: hovered
                QQC2.ToolTip.text: "Предыдущий трек"
            }

            QQC2.ToolButton {
                icon.name: root.playing ? "media-playback-pause" : "media-playback-start"
                flat: true
                onClicked: root.send("PlayPause")
                QQC2.ToolTip.visible: hovered
                QQC2.ToolTip.text: root.playing ? "Пауза" : "Играть"
            }

            QQC2.ToolButton {
                icon.name: "media-skip-forward"
                flat: true
                onClicked: root.send("Next")
                QQC2.ToolTip.visible: hovered
                QQC2.ToolTip.text: "Следующий трек"
            }
        }
    }
}
