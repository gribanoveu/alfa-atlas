const WINDOWS1251_HIGH =
  "ЂЃ‚ѓ„…†‡€‰Љ‹ЊЌЋЏђ‘’“”•–—˜™љ›њќћџ ЎўЈ¤Ґ¦§Ё©Є«¬­®Ї°±Ііґµ¶·ё№є»јЅѕї" +
  "АБВГДЕЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮЯабвгдежзийклмнопрстуфхцчшщъыьэюя";

export function encodeWindows1251ForTest(text: string): Uint8Array {
  const bytes: number[] = [];
  for (const char of text) {
    const code = char.charCodeAt(0);
    if (code < 128) {
      bytes.push(code);
      continue;
    }

    const index = WINDOWS1251_HIGH.indexOf(char);
    if (index === -1) {
      throw new Error(`Символ не представим в Windows-1251: ${char}`);
    }
    bytes.push(index + 128);
  }
  return Uint8Array.from(bytes);
}
