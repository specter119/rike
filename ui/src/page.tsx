import { computed, Scroll, View } from "nsh";
import { DocView } from "./doc";
import { Journals } from "./journals";
import { view } from "./store";

export function Page(props: { rel: string }) {
  return <DocView rel={props.rel} linkTitle={false} withBacklinks />;
}

/** The main area: the Journals stream or the page of the current view. */
export function CurrentView() {
  const rels = computed(() => {
    const v = view();
    return v.kind === "page" ? [v.rel] : [];
  });
  return (
    <Scroll style={{ flex: 1 }}>
      <View style={{ width: "100%", maxWidth: 860, padding: 16, flexDirection: "column" }}>
        <If when={view().kind === "journals"}>
          <Journals />
        </If>
        <Each of={rels()} key={(r) => r}>
          {(rel) => <Page rel={rel()} />}
        </Each>
      </View>
    </Scroll>
  );
}
