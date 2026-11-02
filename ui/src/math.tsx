import { colorScheme, Image, Text, View } from "nsh";
import type { TextViewMath } from "nsh/base";
import { Rike } from "rike";

/** TextView's `renderMath`: a formula as an image, or its TeX source when it does not compile. */
export function renderMath(math: TextViewMath) {
	const f = Rike.renderMath(math.tex, math.display, colorScheme() === "dark");
	if (!f) {
		const tex = (
			<Text
				testID="math-fallback"
				class="text-muted-foreground"
				style={{ fontFamily: "monospace", fontSize: 13 }}
			>
				{math.tex}
			</Text>
		);
		return math.display ? (
			<View style={{ paddingVertical: 8 }}>{tex}</View>
		) : (
			tex
		);
	}
	if (math.display) {
		return (
			<View
				style={{
					width: "100%",
					flexDirection: "row",
					justifyContent: "center",
					marginVertical: 6,
				}}
			>
				<Image
					testID="math"
					src={f.uri}
					style={{
						width: f.width,
						maxWidth: "100%",
						aspectRatio: f.width / f.height,
					}}
				/>
			</View>
		);
	}
	// The image sits on the text baseline by its bottom edge: move it down until the
	// formula's own baseline is there.
	return (
		<Image
			testID="math"
			src={f.uri}
			style={{
				width: f.width,
				height: f.height,
				translate: [0, f.height - f.baseline],
			}}
		/>
	);
}
