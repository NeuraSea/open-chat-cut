"use client";

import { motion, useInView, useReducedMotion } from "motion/react";
import { useRef } from "react";

const SLOT_EASE = [0.62, 0.05, 0, 1] as const;

function SlotCharacter({ char, index }: { char: string; index: number }) {
	const copies = index + 5;
	return <span className="slot-char">
		<span className="slot-char-size">{char}</span>
		<span className="slot-char-mask"><motion.span
			className="slot-char-column"
			initial={{ y: 0 }}
			animate={{ y: `-${(copies - 1) * 1.06}em` }}
			transition={{ duration: .52 + index * .028, delay: index * .018, ease: SLOT_EASE }}
		>{Array.from({ length: copies }, (_, copy) => <span className="slot-char-glyph" key={copy}>{char}</span>)}</motion.span></span>
	</span>;
}

export function SlotText({ text, className }: { text: string; className?: string }) {
	const ref = useRef<HTMLSpanElement>(null);
	const inView = useInView(ref, { once: true, margin: "-8%" });
	const reducedMotion = useReducedMotion();
	let index = 0;

	return <h1 className={className}>
		<span className="sr-only">{text}</span>
		<span aria-hidden="true" className="slot-lines" ref={ref}>
			{text.split("\n").map((line, lineIndex) => <span className="slot-line" key={`${line}-${lineIndex}`}>
				{(/\p{Script=Han}/u.test(line) ? Array.from(line) : line.split(/(\s+)/)).map((token, tokenIndex) => token.trim() === ""
					? <span className="slot-space" key={`space-${tokenIndex}`} />
					: <span className="slot-word" key={`${token}-${tokenIndex}`}>{Array.from(token).map(char => {
						const charIndex = index++;
						return inView && !reducedMotion ? <SlotCharacter char={char} index={charIndex} key={`${char}-${charIndex}`} /> : <span className="slot-static" key={`${char}-${charIndex}`}>{char}</span>;
					})}</span>) }
			</span>)}
		</span>
	</h1>;
}
