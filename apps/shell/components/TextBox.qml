import QtQuick
import "../theme"

// Single-line text field (plain TextInput: no Qt Quick Controls dependency). Typing needs
// the on-screen keyboard, which squeekboard shows when the field takes focus. `source`
// is the stored value: the field shows it whenever it is not being edited.
Rectangle {
    id: root
    property string source
    property string placeholder
    property alias text: input.text
    property alias maximumLength: input.maximumLength
    readonly property bool editing: input.activeFocus
    signal accepted()
    function clearFocus() { input.focus = false }

    implicitHeight: Theme.touchTarget
    radius: 12
    color: Theme.surfaceRaised
    border.width: input.activeFocus ? 1.5 : 1
    border.color: input.activeFocus ? Theme.accent : Theme.outline
    onSourceChanged: if (!input.activeFocus) input.text = source
    Component.onCompleted: input.text = source

    TextInput {
        id: input
        anchors.fill: parent
        anchors.leftMargin: Theme.space3
        anchors.rightMargin: Theme.space3
        verticalAlignment: TextInput.AlignVCenter
        color: Theme.text
        selectionColor: Theme.accent
        font.pixelSize: Theme.fontBody
        clip: true
        onAccepted: { root.accepted(); focus = false }
        onEditingFinished: root.accepted()
        Text {
            anchors.fill: parent
            verticalAlignment: Text.AlignVCenter
            visible: input.text.length === 0 && !input.activeFocus
            text: root.placeholder
            color: Theme.textDisabled
            font.pixelSize: Theme.fontBody
        }
    }
}
