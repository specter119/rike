import { Text, token, View } from "nsh";
import type { NoticeDto } from "rike";
import { Backlinks } from "./backlinks";
import { Block } from "./block";
import { Btn } from "./widgets";
import { acceptDisk, keepMine, pageOf } from "./store";

function Banner(props: { rel: string; notice: () => NoticeDto }) {
  const external = () => props.notice().kind === "external";
  return (
    <View
      testID="notice"
      class="rounded-md border border-border bg-muted"
      style={{ width: "100%", flexDirection: "row", alignItems: "center", gap: 8, padding: 8, marginBottom: 8 }}
    >
      <View style={{ flex: 1 }}>
        <Text style={{ fontSize: 12 }}>{props.notice().message}</Text>
      </View>
      <Btn label={external() ? "载入外部版本" : "载入磁盘版本"} onPress={() => acceptDisk(props.rel)} />
      <Btn label={external() ? "保留我的" : "覆盖磁盘"} onPress={() => keepMine(props.rel)} />
    </View>
  );
}

/** The outline of a page: its banner and its blocks. */
export function DocBody(props: { rel: string }) {
  const page = pageOf(props.rel);
  const readOnly = () => page()?.readOnly ?? false;
  return (
    <View style={{ flexDirection: "column" }}>
      <If when={page()?.notice}>{(n) => <Banner rel={props.rel} notice={n} />}</If>
      <Each of={page()?.rows ?? []} key={(r) => r.id}>
        {(row) => <Block rel={props.rel} row={row} readOnly={readOnly} />}
      </Each>
    </View>
  );
}

/** A page opened on its own: where it lives, its outline, and who links here. */
export function DocPage(props: { rel: string }) {
  const page = pageOf(props.rel);
  return (
    <View style={{ flexDirection: "column" }}>
      <View style={{ flexDirection: "row", alignItems: "center", gap: 8, marginBottom: 12 }}>
        <Text testID="doc-path" class="text-muted-foreground" style={{ fontSize: 11, fontFamily: token("font-mono") }}>
          {page()?.rel ?? ""}
        </Text>
        <If when={page()?.readOnly}>
          <Text class="text-muted-foreground" style={{ fontSize: 11 }}>
            只读
          </Text>
        </If>
      </View>
      <DocBody rel={props.rel} />
      <Backlinks rel={props.rel} />
    </View>
  );
}
