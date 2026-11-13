import { computed, signal, Text, token, View } from "nsh";
import { Button, Input } from "nsh/base";
import { Rike } from "rike";
import type { SearchHitDto } from "rike";
import { pageLabel } from "./dates";
import { closeSearch, indexVer, navigate, searchOpen, setComposing } from "./store";

function open(hit: SearchHitDto): void {
  navigate({ kind: "page", rel: hit.rel });
}

function label(hit: SearchHitDto): string {
  const page = pageLabel(hit.rel, hit.title);
  if (hit.kind === "page") return page;
  return `${page} — ${(hit.text ?? "").split("\n")[0]}`;
}

function SearchPanel() {
  // Made on every open, so the box always starts empty.
  const query = signal("");
  const hits = computed(() => {
    indexVer();
    try {
      return Rike.search(query()).map((hit, n) => ({ hit, n }));
    } catch {
      return [];
    }
  });
  return (
    <View
      testID="search"
      class="border-b border-border bg-card"
      style={{ flexDirection: "column", gap: 2, paddingHorizontal: 32, paddingTop: 12, paddingBottom: 12 }}
    >
      <Input
        testID="search-input"
        placeholder="搜索页面和块…"
        autoFocus
        onChange={(e) => query.set(e.value)}
        onCompositionChange={(e) => setComposing(e.composing)}
        onSubmit={() => {
          const first = hits()[0];
          if (first) open(first.hit);
        }}
        style={{ width: "100%", padding: 4, fontSize: 14 }}
      />
      <Each of={hits()} key={(h) => h.n}>
        {(h) => (
          <Button
            testID="search-hit"
            onPress={() => open(h().hit)}
            class="rounded-md hover:bg-muted"
            style={{ paddingHorizontal: 8, paddingVertical: 4 }}
          >
            <Text style={{ fontSize: 13 }}>{label(h().hit)}</Text>
          </Button>
        )}
      </Each>
      <If when={hits().length === 0}>
        <Text class="text-muted-foreground" style={{ fontSize: 11, paddingHorizontal: 8, paddingTop: 2, fontFamily: token("font-mono") }}>
          Ctrl K 打开，Esc 关闭
        </Text>
      </If>
    </View>
  );
}

export function SearchOverlay() {
  return (
    <If when={searchOpen()}>
      <SearchPanel />
    </If>
  );
}

export { closeSearch };
