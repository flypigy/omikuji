import QtQuick
import QtQuick.Controls
import omikuji 1.0
import "../lib/Nav.js" as Nav

// Flow+Repeater not GridView becuase GridView only repositions on model changes, we need the slide when a card flips visible false for filtering
Item {
    id: root

    // windowed GridView kicks in only when the consumer supplies modelCount and it
    // exceeds virtualizeThreshold; consumers on the Flow path are unaffected
    property var model: null
    property int modelCount: -1
    property int virtualizeThreshold: 2147483647
    // uniform delegate height for the virtual path; 0 derives from cardBaseHeight
    property real virtualCardHeight: 0
    readonly property bool virtualized: modelCount >= 0 && modelCount > virtualizeThreshold
    readonly property int count: root.virtualized ? virtualView.count : repeater.count
    // delegates must bind their width to this, not 180*zoom: the fractional product
    // can exceed the flow's rounded column math and push the last column onto a new row
    readonly property int cardPixelWidth: Math.round(root.cardBaseWidth * root.cardZoom)
    property Component delegate: null

    property Component headerComponent: null
    property int headerHeight: 36
    property int headerTopMargin: 12
    property int headerSideMargin: 20
    property int headerSpacing: 4

    property real cardZoom: 1.0
    property int cardSpacing: 16
    property int cardBaseWidth: 180
    property int cardBaseHeight: 240
    property string cardFlow

    // library uses this to clear selection, stores ignore it
    signal backgroundClicked()

    property int keyIndex: -1
    readonly property bool navFocused: gridFlick.activeFocus
    readonly property int columns: grid.colsToUse

    // the first layout after a reveal places cards built while hidden
    property bool _laidOutVisible: false
    onVisibleChanged: if (!visible) _laidOutVisible = false

    signal keyNavMoved(int index)
    signal keyNavActivated(int index)

    property bool reorderEnabled: false
    readonly property bool reordering: keyReorder.lifted
    signal reorderMoveRequested(int from, int to)
    signal reorderCommitted()

    function liftAt(index) {
        if (!root.reorderEnabled) return false
        root.keyIndex = index
        gridFlick.forceActiveFocus(Qt.TabFocusReason)
        keyReorder.lift()
        return true
    }

    SmoothScroll {
        id: scroller
        flick: gridFlick
    }

    KeyReorder {
        id: keyReorder
        index: root.keyIndex
        count: repeater.count
        rowStep: root.columns
        horizontal: true
        onMoveRequested: (from, to) => {
            scroller.revealItem(repeater.itemAt(to), 20)
            root.reorderMoveRequested(from, to)
        }
        onCommitted: root.reorderCommitted()
    }

    function itemAt(i) {
        return root.virtualized ? virtualView.itemAtIndex(i) : repeater.itemAt(i)
    }

    function focusIndex(i) {
        keyIndex = i
        if (root.virtualized) {
            virtualView.currentIndex = i
            virtualView.positionViewAtIndex(i, GridView.Contain)
            virtualView.forceActiveFocus(Qt.TabFocusReason)
        } else {
            gridFlick.forceActiveFocus()
        }
    }

    function _visibleIndices() {
        const out = []
        for (let i = 0; i < repeater.count; i++) {
            const it = repeater.itemAt(i)
            if (it && it.cardVisible !== false) out.push(i)
        }
        return out
    }

    function _onScreen(card) {
        const top = card.mapToItem(gridFlick, 0, 0).y
        return top + card.height > 0 && top < gridFlick.height
    }

    function _moveTo(i) {
        keyIndex = i
        Nav.ensureVisible(gridFlick, repeater.itemAt(i), 20)
        keyNavMoved(i)
    }

    function _step(pos, count, key) {
        const col = pos % columns
        switch (key) {
        case Qt.Key_Left: return col === 0 ? -1 : pos - 1
        case Qt.Key_Right: return col === columns - 1 || pos === count - 1 ? -1 : pos + 1
        case Qt.Key_Up: return pos - columns
        case Qt.Key_Down:
            if (pos + columns < count) return pos + columns
            return Math.floor(pos / columns) < Math.floor((count - 1) / columns) ? count - 1 : -1
        }
        return -1
    }

    function _activate(i) {
        const it = repeater.itemAt(i)
        if (it && typeof it.primaryAction === "function") it.primaryAction()
        else keyNavActivated(i)
    }

    function _handleKey(event) {
        const vis = _visibleIndices()
        const pos = vis.indexOf(keyIndex)
        const isArrow = [Qt.Key_Left, Qt.Key_Right, Qt.Key_Up, Qt.Key_Down].includes(event.key)
        if (!isArrow || vis.length === 0) {
            event.accepted = false
            return
        }
        const next = pos === -1 ? 0 : _step(pos, vis.length, event.key)
        event.accepted = next >= 0
        if (event.accepted) _moveTo(vis[next])
    }

    Loader {
        id: header
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.topMargin: root.headerTopMargin
        anchors.leftMargin: root.headerSideMargin
        anchors.rightMargin: root.headerSideMargin
        height: active ? root.headerHeight : 0
        sourceComponent: root.headerComponent
        active: sourceComponent !== null
    }

    Flickable {
        id: gridFlick
        anchors.top: header.active ? header.bottom : parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.topMargin: header.active ? root.headerSpacing : 0
        contentHeight: grid.height + 100
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        flickDeceleration: 3000
        maximumFlickVelocity: 1500

        // wheel interception must sit ABOVE the cards or the flickable's native
        // wheel handling wins. NoButton keeps clicks pass-through. one card row
        // per notch, lightly animated, clamped to the content bounds.
        MouseArea {
            anchors.fill: parent
            z: 2
            acceptedButtons: Qt.NoButton
            onWheel: (wheel) => {
                const rowH = root.cardBaseHeight * root.cardZoom + root.cardSpacing
                const max = Math.max(0, gridFlick.contentHeight - gridFlick.height)
                const base = wheelAnim.running ? wheelAnim.to : gridFlick.contentY
                const to = Math.max(0, Math.min(max, base - wheel.angleDelta.y * rowH / 120))
                wheelAnim.stop()
                wheelAnim.to = to
                wheelAnim.start()
                wheel.accepted = true
            }
        }
        NumberAnimation {
            id: wheelAnim
            target: gridFlick
            property: "contentY"
            duration: 120
            easing.type: Theme.ease.standard
        }

        readonly property bool navigable: repeater.count > 0
        function navRectItem() {
            const vis = root._visibleIndices()
            if (vis.length === 0) return null
            return repeater.itemAt(vis.indexOf(root.keyIndex) !== -1 ? root.keyIndex : vis[0])
        }
        function navTargets() {
            return root._visibleIndices().map(i => repeater.itemAt(i)).filter(root._onScreen)
        }
        function navEnter(card) {
            if (card.index !== root.keyIndex) root._moveTo(card.index)
        }
        function navActivate() {
            if (root._visibleIndices().includes(root.keyIndex)) root._activate(root.keyIndex)
        }
        Keys.onShortcutOverride: (event) => keyReorder.handleShortcutOverride(event)
        Keys.onPressed: (event) => keyReorder.lifted ? keyReorder.handleKey(event) : root._handleKey(event)
        onActiveFocusChanged: if (!activeFocus) keyReorder.drop(false)

        ScrollBar.vertical: ThinScrollBar { padding: 4 }

        // covers the whole scrollable area so clicks on empty flow space falls through
        MouseArea {
            width: parent.width
            height: Math.max(grid.height + 100, gridFlick.height)
            z: 0
            onClicked: {
                forceActiveFocus()
                root.backgroundClicked()
            }
        }

        Flow {
            id: grid
            y: 20
            spacing: root.cardSpacing
            flow: Flow.LeftToRight
            z: 1

            // width shrinks to exactly N full cards so the last partial row sits left-aligned
            property int cardW: root.cardPixelWidth
            readonly property int sidePad: 12
            property int avail: Math.max(0, parent.width - sidePad * 2)
            property int maxCols: Math.max(1, Math.floor((avail + root.cardSpacing) / (cardW + root.cardSpacing)))
            property int colsToUse: Math.max(1, Math.min(maxCols, repeater.count))
            // a Flow never reflows children hidden while it was, the spare pixel forces it on show
            width: colsToUse * cardW + (colsToUse - 1) * root.cardSpacing + (root.visible ? 0 : 1)
            x: {
                if (root.cardFlow === "center") return Math.max(sidePad, (parent.width - width) / 2)
                if (root.cardFlow === "right") return Math.max(sidePad, parent.width - width - sidePad)
                return sidePad
            }

            onPositioningComplete: root._laidOutVisible = root.visible

            move: Transition {
                enabled: root._laidOutVisible
                NumberAnimation { properties: "x,y"; duration: Theme.dur.lg; easing.type: Theme.ease.standard }
            }

            Repeater {
                id: repeater
                model: root.virtualized ? null : root.model
                delegate: root.delegate
                onItemAdded: (index, item) => {
                    if (item.keyFocused !== undefined)
                        item.keyFocused = Qt.binding(() => InputMode.keyboard && root.navFocused && root.keyIndex === item.index)
                }
            }
        }
    }

    // windowed path for huge libraries: only on-screen cards (+cacheBuffer) exist,
    // pooled via reuseItems. no per-card filter animation or reorder here — big store
    // pages filter at the model layer and libraries never cross the threshold.
    GridView {
        id: virtualView

        anchors.top: header.active ? header.bottom : parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.topMargin: header.active ? root.headerSpacing : 0

        visible: root.virtualized
        enabled: root.virtualized
        clip: true
        model: root.virtualized ? root.model : null
        delegate: root.delegate

        reuseItems: true
        boundsBehavior: Flickable.StopAtBounds
        flickDeceleration: 3000
        maximumFlickVelocity: 1500

        // same wheel interception as the flow grid
        MouseArea {
            anchors.fill: parent
            z: 5
            acceptedButtons: Qt.NoButton
            onWheel: (wheel) => {
                const rowH = virtualView.cardH + root.cardSpacing
                const max = Math.max(0, virtualView.contentHeight - virtualView.height)
                const base = virtualWheelAnim.running ? virtualWheelAnim.to : virtualView.contentY
                const to = Math.max(0, Math.min(max, base - wheel.angleDelta.y * rowH / 120))
                virtualWheelAnim.stop()
                virtualWheelAnim.to = to
                virtualWheelAnim.start()
                wheel.accepted = true
            }
        }
        NumberAnimation {
            id: virtualWheelAnim
            target: virtualView
            property: "contentY"
            duration: 120
            easing.type: Theme.ease.standard
        }

        readonly property int cardW: root.cardPixelWidth
        readonly property int cardH: root.virtualCardHeight > 0
            ? Math.round(root.virtualCardHeight)
            : Math.round(root.cardBaseHeight * root.cardZoom)
        cellWidth: cardW + root.cardSpacing
        cellHeight: cardH + root.cardSpacing

        ScrollBar.vertical: ThinScrollBar { padding: 4 }

        // covers the whole scrollable area so clicks on empty grid space fall through
        MouseArea {
            anchors.fill: parent
            z: -1
            onClicked: {
                forceActiveFocus()
                root.backgroundClicked()
            }
        }

        Keys.onReturnPressed: (event) => {
            event.accepted = true
            root.keyNavActivated(currentIndex)
        }
        Keys.onEnterPressed: (event) => {
            event.accepted = true
            root.keyNavActivated(currentIndex)
        }
    }
}
