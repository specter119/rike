import { View } from "nsh";
import { DocView } from "./doc";
import { journals, loadMoreJournals } from "./store";
import { Btn } from "./widgets";

export function Journals() {
  return (
    <View testID="journals" style={{ flexDirection: "column", gap: 16 }}>
      <Each of={journals().entries} key={(e) => e.rel}>
        {(entry) => (
          <View testID="journal-card" class="rounded-lg border border-border" style={{ padding: 12 }}>
            <DocView rel={entry().rel} linkTitle withBacklinks={false} />
          </View>
        )}
      </Each>
      <If when={journals().more}>
        <Btn label="加载更多" onPress={loadMoreJournals} />
      </If>
    </View>
  );
}
