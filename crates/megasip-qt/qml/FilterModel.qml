import QtQuick
import QtQml.Models

// A filtered view of a model, for a ListView: set `model`, `delegate` and `accept`, a
// function from a row (its roles) to whether to show it, and call invalidateFilter() when
// what `accept` looks at changes. Rows added to the model are filtered as they arrive.
// In the delegate, the row's position in the view is DelegateModel.shownIndex.
DelegateModel {
    id: fm
    property var accept: row => true

    groups: DelegateModelGroup { id: shown; name: "shown"; includeByDefault: false }
    filterOnGroup: "shown"

    function filterRange(from, to) {
        for (let i = from; i < to; i++) {
            const it = items.get(i)
            const want = !!accept(it.model)
            if (it.inShown !== want) it.inShown = want
        }
    }
    function invalidateFilter() { filterRange(0, items.count) }
    // The shown rows, in order.
    function shownCount() { return shown.count }
    function shownRow(i) { return shown.get(i).model }

    property Connections _arrivals: Connections {
        target: fm.items
        function onChanged(removed, inserted) {
            for (let k = 0; k < inserted.length; k++)
                fm.filterRange(inserted[k].index, inserted[k].index + inserted[k].count)
        }
    }
    onAcceptChanged: invalidateFilter()
}
