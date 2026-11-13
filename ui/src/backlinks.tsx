import { Text, token, View } from "nsh";
import { Button, TextView } from "nsh/base";
import { onLinkPress } from "./block";
import { pageLabel } from "./dates";
import { renderMath } from "./math";
import { backlinksOf, indexing, navigate } from "./store";

export function Backlinks(props: { rel: string }) {
  const groups = backlinksOf(props.rel);
  return (
    <View
      testID="backlinks"
      style={{
        flexDirection: "column",
        marginTop: 28,
        paddingTop: 16,
        borderTopWidth: 1,
        borderColor: token("border"),
      }}
    >
      <If when={groups().length > 0}>
        <Text class="text-muted-foreground" style={{ fontSize: 11, marginBottom: 12 }}>
          {`${groups().length} 个页面链接到这里`}
        </Text>
      </If>
      <If when={groups().length === 0 && indexing()}>
        <Text class="text-muted-foreground" style={{ fontSize: 11, marginBottom: 12 }}>
          索引中…
        </Text>
      </If>
      <Each of={groups()} key={(g) => g.rel}>
        {(group) => (
          <View style={{ flexDirection: "column", marginBottom: 16 }}>
            <Button
              onPress={() => navigate({ kind: "page", rel: group().rel })}
              style={{ flexDirection: "row", marginBottom: 4 }}
            >
              <Text style={{ fontSize: 13, fontWeight: "600" }}>{pageLabel(group().rel, group().title)}</Text>
            </Button>
            <Each of={group().items} key={(item) => item.block}>
              {(item) => (
                <View style={{ flexDirection: "row", gap: 10 }}>
                  <View style={{ width: 1, backgroundColor: token("border") }} />
                  <View style={{ flex: 1, flexDirection: "column", paddingVertical: 2 }}>
                    <If when={item().ancestors.length > 0}>
                      <Text class="text-muted-foreground" style={{ fontSize: 11 }}>
                        {item().ancestors.join(" › ")}
                      </Text>
                    </If>
                    <TextView markdown={item().markdown} renderMath={renderMath} onLinkPress={onLinkPress} />
                  </View>
                </View>
              )}
            </Each>
          </View>
        )}
      </Each>
    </View>
  );
}
