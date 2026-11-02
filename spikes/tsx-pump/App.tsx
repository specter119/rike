import { mount, signal, Text, View } from "nsh";
import { Badge, Pump } from "probe/native";

function App() {
  const a = signal(0);
  const b = signal(0);
  const starts = signal(0);
  let stopA: (() => void) | null = Pump.onEvent(() => a.set(a() + 1));
  Pump.run();
  starts.set(Pump.starts());
  return (
    <View style={{ flex: 1, gap: 8 }}>
      <Text>{`a ${a()}`}</Text>
      <Text>{`b ${b()}`}</Text>
      <Text>{`starts ${starts()}`}</Text>
      <Badge label="swap" onPress={() => {
        // hot-update style: new registration first, then the old Stop runs late
        const old = stopA;
        Pump.onEvent(() => b.set(b() + 1));
        old?.();
        stopA = null;
      }} />
      <Badge label="unreg-a" onPress={() => { stopA?.(); stopA = null; }} />
      <Badge label="reg-b" onPress={() => { Pump.onEvent(() => b.set(b() + 1)); }} />
    </View>
  );
}

mount(App);
