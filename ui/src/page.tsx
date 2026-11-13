import { computed, Scroll, Text, token, View } from "nsh";
import { pageLabel } from "./dates";
import { DocPage } from "./doc";
import { Journals } from "./journals";
import { back, canBack, canForward, forward, indexProgress, indexing, pageOf, view } from "./store";
import { WindowControls } from "./window";
import { GlyphButton } from "./widgets";

/** What the view is, how to go back, and the window's own controls. */
function Header() {
  const title = () => {
    const v = view();
    if (v.kind === "journals") return "日记";
    const page = pageOf(v.rel)();
    return page ? pageLabel(v.rel, page.title) : "";
  };
  return (
    <View
      style={{
        minHeight: 56,
        flexShrink: 0,
        flexDirection: "row",
        alignItems: "center",
        gap: 2,
        paddingLeft: 22,
        paddingRight: 10,
        borderBottomWidth: 1,
        borderColor: token("border"),
      }}
    >
      <GlyphButton testID="nav-back" glyph="←" label="后退" disabled={!canBack()} onPress={back} />
      <GlyphButton testID="nav-forward" glyph="→" label="前进" disabled={!canForward()} onPress={forward} />
      <View style={{ width: 10 }} />
      <View style={{ flexShrink: 1 }}>
        <Text style={{ fontFamily: token("font-serif"), fontSize: 18, textOverflow: "ellipsis" }}>
          {title()}
        </Text>
      </View>
      <View style={{ flex: 1 }} />
      <WindowControls />
    </View>
  );
}

/** How far the background index got; a hairline that fills as it parses. */
function IndexBar() {
  const percent = () => {
    const p = indexProgress();
    return p.total === 0 ? 0 : Math.round((p.done / p.total) * 100);
  };
  return (
    <If when={indexing()}>
      <View style={{ height: 2, backgroundColor: token("muted") }}>
        <View style={{ height: 2, width: `${percent()}%`, backgroundColor: token("seal") }} />
      </View>
    </If>
  );
}

/** The main area: the Journals stream or the page of the current view. */
export function CurrentView() {
  const rels = computed(() => {
    const v = view();
    return v.kind === "page" ? [v.rel] : [];
  });
  return (
    <View style={{ flex: 1, flexDirection: "column" }}>
      <Header />
      <IndexBar />
      <Scroll style={{ flex: 1 }}>
        <View
          style={{
            width: "100%",
            maxWidth: 920,
            paddingHorizontal: 36,
            paddingBottom: 72,
            flexDirection: "column",
          }}
        >
          <If when={view().kind === "journals"}>
            <Journals />
          </If>
          <Each of={rels()} key={(r) => r}>
            {(rel) => <DocPage rel={rel()} />}
          </Each>
        </View>
      </Scroll>
    </View>
  );
}
