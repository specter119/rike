import { Text, token, View } from "nsh";
import { Button } from "nsh/base";
import { dayOf, monthName, weekdayOf } from "./dates";
import { DocBody } from "./doc";
import { journals, loadMoreJournals, navigate } from "./store";
import { Btn } from "./widgets";

/** A day of the stream: its date in the margin, its outline on the page. */
function Day(props: { rel: string; first: boolean; today: boolean }) {
  const day = () => dayOf(props.rel);
  return (
    <View
      testID="journal-card"
      style={{
        flexDirection: "row",
        gap: 24,
        paddingTop: 26,
        paddingBottom: 30,
        borderTopWidth: props.first ? 0 : 1,
        borderColor: token("border"),
      }}
    >
      <View style={{ width: 76, alignItems: "flex-end" }}>
        <If when={day()}>
          {(d) => (
            <Button
              testID="doc-title"
              onPress={() => navigate({ kind: "page", rel: props.rel })}
              class="rounded-md hover:bg-muted"
              style={{ alignItems: "flex-end", paddingHorizontal: 4, paddingVertical: 2 }}
            >
              <Text
                style={{
                  fontFamily: token("font-serif"),
                  fontSize: 28,
                  lineHeight: 30,
                  color: props.today ? token("seal") : token("foreground"),
                }}
              >
                {String(d().day)}
              </Text>
              <Text class="text-muted-foreground" style={{ fontSize: 11, marginTop: 6 }}>
                {`${monthName(d().month)} ${weekdayOf(d())}`}
              </Text>
            </Button>
          )}
        </If>
      </View>
      <View style={{ flex: 1 }}>
        <DocBody rel={props.rel} />
      </View>
    </View>
  );
}

export function Journals() {
  return (
    <View testID="journals" style={{ flexDirection: "column" }}>
      <Each of={journals().entries} key={(e) => e.rel}>
        {(entry, index) => <Day rel={entry().rel} first={index() === 0} today={index() === 0} />}
      </Each>
      <If when={journals().more}>
        <View style={{ marginTop: 20 }}>
          <Btn label="加载更多" onPress={loadMoreJournals} />
        </View>
      </If>
    </View>
  );
}
