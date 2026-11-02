import { Text, View } from "nsh";
import { dismissToast, toasts } from "./store";
import { Btn } from "./widgets";

/** Info and error messages. Page-level banners (external change, conflict) live in the page. */
export function Toasts() {
  return (
    <View style={{ width: "100%", flexDirection: "column" }}>
      <Each of={toasts()} key={(n) => n.id}>
        {(notice) => (
          <View
            testID="notice"
            class="bg-muted"
            style={{ width: "100%", flexDirection: "row", alignItems: "center", gap: 8, paddingHorizontal: 12, paddingVertical: 4 }}
          >
            <View style={{ flex: 1 }}>
              <Text style={{ fontSize: 12 }}>{notice().message}</Text>
            </View>
            <Btn label="关闭" onPress={() => dismissToast(notice().id)} />
          </View>
        )}
      </Each>
    </View>
  );
}
