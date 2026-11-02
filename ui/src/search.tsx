import { computed, signal, Text, View } from "nsh";
import { Button, Input } from "nsh/base";
import { Rike } from "rike";
import type { SearchHitDto } from "rike";
import { closeSearch, indexVer, navigate, searchOpen, setComposing } from "./store";

function open(hit: SearchHitDto): void {
  navigate({ kind: "page", rel: hit.rel });
}

function label(hit: SearchHitDto): string {
  if (hit.kind === "page") return `📄 ${hit.title}`;
  return `${hit.title} — ${(hit.text ?? "").split("\n")[0]}`;
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
    <View testID="search" class="border-b border-border bg-muted" style={{ flexDirection: "column", gap: 4, padding: 8 }}>
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
          <Button testID="search-hit" onPress={() => open(h().hit)} class="rounded" style={{ paddingHorizontal: 8, paddingVertical: 4 }}>
            <Text style={{ fontSize: 13 }}>{label(h().hit)}</Text>
          </Button>
        )}
      </Each>
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
