import { Text, View } from "nsh";
import { Button, TextView } from "nsh/base";
import { onLinkPress } from "./block";
import { renderMath } from "./math";
import { backlinksOf, navigate } from "./store";

export function Backlinks(props: { rel: string }) {
  const groups = backlinksOf(props.rel);
  return (
    <View testID="backlinks" style={{ flexDirection: "column", gap: 4, marginTop: 16 }}>
      <If when={groups().length > 0}>
        <Text class="text-muted-foreground" style={{ fontSize: 13, fontWeight: "600" }}>
          {`${groups().length} 个页面链接到这里`}
        </Text>
      </If>
      <Each of={groups()} key={(g) => g.rel}>
        {(group) => (
          <View class="rounded-md bg-muted" style={{ flexDirection: "column", gap: 4, padding: 8 }}>
            <Button onPress={() => navigate({ kind: "page", rel: group().rel })}>
              <Text style={{ fontSize: 14, fontWeight: "600" }}>{group().title}</Text>
            </Button>
            <Each of={group().items} key={(item) => item.block}>
              {(item) => (
                <View style={{ flexDirection: "column" }}>
                  <Text class="text-muted-foreground" style={{ fontSize: 11 }}>
                    {item().ancestors.join(" › ")}
                  </Text>
                  <TextView markdown={item().markdown} renderMath={renderMath} onLinkPress={onLinkPress} />
                </View>
              )}
            </Each>
          </View>
        )}
      </Each>
    </View>
  );
}
