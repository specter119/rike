import { Text, View } from "nsh";
import { Button } from "nsh/base";
import type { NoticeDto } from "rike";
import { Backlinks } from "./backlinks";
import { Block } from "./block";
import { Btn } from "./widgets";
import { acceptDisk, keepMine, navigate, pageOf } from "./store";

function Banner(props: { rel: string; notice: () => NoticeDto }) {
  const external = () => props.notice().kind === "external";
  return (
    <View testID="notice" class="bg-muted" style={{ width: "100%", flexDirection: "row", alignItems: "center", gap: 8, padding: 8 }}>
      <View style={{ flex: 1 }}>
        <Text style={{ fontSize: 12 }}>{props.notice().message}</Text>
      </View>
      <Btn label={external() ? "载入外部版本" : "载入磁盘版本"} onPress={() => acceptDisk(props.rel)} />
      <Btn label={external() ? "保留我的" : "覆盖磁盘"} onPress={() => keepMine(props.rel)} />
    </View>
  );
}

/** A page: title, blocks and, for a page view, its backlinks. */
export function DocView(props: { rel: string; linkTitle: boolean; withBacklinks: boolean }) {
  const page = pageOf(props.rel);
  const readOnly = () => page()?.readOnly ?? false;
  return (
    <View style={{ flexDirection: "column", gap: 4 }}>
      <Button
        onPress={() => {
          if (props.linkTitle) navigate({ kind: "page", rel: props.rel });
        }}
        style={{ flexDirection: "row", gap: 8, alignItems: "center" }}
      >
        <Text testID="doc-title" style={{ fontSize: 22, fontWeight: "600" }}>
          {page()?.title ?? ""}
        </Text>
        <If when={readOnly()}>
          <Text class="text-muted-foreground" style={{ fontSize: 11 }}>
            只读
          </Text>
        </If>
      </Button>
      <If when={props.linkTitle}>
        <Text testID="doc-path" class="text-muted-foreground" style={{ fontSize: 11 }}>
          {page()?.rel ?? ""}
        </Text>
      </If>
      <If when={page()?.notice}>{(n) => <Banner rel={props.rel} notice={n} />}</If>
      <Each of={page()?.rows ?? []} key={(r) => r.id}>
        {(row) => <Block rel={props.rel} row={row} readOnly={readOnly} />}
      </Each>
      <If when={props.withBacklinks}>
        <Backlinks rel={props.rel} />
      </If>
    </View>
  );
}

