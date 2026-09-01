import { afterEach, describe, expect, test } from "bun:test";
import {
  applyFieldEdit,
  asTextField,
  availabilityFor,
  fieldSelectionText,
  isFieldReadable,
  isFieldWritable,
  isInsideMonaco,
  spliceValue,
  selectionTextOf,
  type EditTarget,
} from "../lib/editClipboard";

function input(type: string, value: string): HTMLInputElement {
  const el = document.createElement("input");
  el.type = type;
  el.value = value;
  document.body.append(el);
  return el;
}

afterEach(() => {
  document.body.innerHTML = "";
});

describe("asTextField", () => {
  test("принимает textarea и текстовые input", () => {
    const area = document.createElement("textarea");
    expect(asTextField(area)).toBe(area);
    const text = input("text", "");
    expect(asTextField(text)).toBe(text);
    expect(asTextField(input("password", ""))).not.toBeNull();
  });

  test("отвергает типы без selectionStart и не-поля", () => {
    expect(asTextField(input("number", "1"))).toBeNull();
    expect(asTextField(input("checkbox", ""))).toBeNull();
    expect(asTextField(document.createElement("div"))).toBeNull();
    expect(asTextField(null)).toBeNull();
  });
});

test("isInsideMonaco находит скрытую textarea редактора", () => {
  const host = document.createElement("div");
  host.className = "monaco-editor";
  const area = document.createElement("textarea");
  host.append(area);
  document.body.append(host);

  expect(isInsideMonaco(area)).toBe(true);
  expect(isInsideMonaco(input("text", ""))).toBe(false);
});

describe("выделение в поле", () => {
  test("пустое выделение даёт пустую строку", () => {
    const el = input("text", "привет");
    el.setSelectionRange(2, 2);
    expect(fieldSelectionText(el)).toBe("");
  });

  test("выделенный диапазон", () => {
    const el = input("text", "привет");
    el.setSelectionRange(0, 3);
    expect(fieldSelectionText(el)).toBe("при");
  });
});

test("spliceValue заменяет диапазон и двигает курсор", () => {
  expect(spliceValue("abcdef", 1, 3, "XY")).toEqual({ value: "aXYdef", caret: 3 });
  expect(spliceValue("abcdef", 1, 3, "")).toEqual({ value: "adef", caret: 1 });
});

test("applyFieldEdit меняет значение и шлёт input-событие", () => {
  const el = input("text", "abcdef");
  el.setSelectionRange(1, 3);

  let events = 0;
  el.addEventListener("input", () => {
    events += 1;
  });

  applyFieldEdit(el, "XY");

  expect(el.value).toBe("aXYdef");
  expect(el.selectionStart).toBe(3);
  expect(events).toBe(1);
});

test("вырезание из поля — это вставка пустой строки", () => {
  const el = input("text", "abcdef");
  el.setSelectionRange(2, 4);
  applyFieldEdit(el, "");
  expect(el.value).toBe("abef");
});

describe("availabilityFor", () => {
  test("без цели доступно только копирование выделения на странице", () => {
    expect(availabilityFor(null, "текст")).toEqual({ cut: false, copy: true, paste: false });
    expect(availabilityFor(null, "")).toEqual({ cut: false, copy: false, paste: false });
  });

  test("поле с выделением: всё доступно", () => {
    const el = input("text", "abcdef");
    el.setSelectionRange(0, 2);
    expect(availabilityFor({ kind: "field", el }, "")).toEqual({
      cut: true,
      copy: true,
      paste: true,
    });
  });

  test("поле без выделения: только вставка", () => {
    const el = input("text", "abcdef");
    el.setSelectionRange(1, 1);
    expect(availabilityFor({ kind: "field", el }, "")).toEqual({
      cut: false,
      copy: false,
      paste: true,
    });
  });

  test("readonly и disabled поля не принимают вставку", () => {
    const readonly = input("text", "abc");
    readonly.readOnly = true;
    readonly.setSelectionRange(0, 3);
    expect(isFieldWritable(readonly)).toBe(false);
    expect(availabilityFor({ kind: "field", el: readonly }, "")).toEqual({
      cut: false,
      copy: true,
      paste: false,
    });
  });

  test("пароль не отдаёт текст в буфер", () => {
    const el = input("password", "secret");
    el.setSelectionRange(0, 6);
    expect(isFieldReadable(el)).toBe(false);
    expect(availabilityFor({ kind: "field", el }, "")).toEqual({
      cut: false,
      copy: false,
      paste: true,
    });
    expect(selectionTextOf({ kind: "field", el }, "")).toBe("");
  });

  test("редактор только для чтения копируется, но не правится", () => {
    const target = {
      kind: "monaco",
      editor: {
        getRawOptions: () => ({ readOnly: true }),
        getModel: () => ({ getValueInRange: () => "выделенное" }),
        getSelection: () => ({ isEmpty: () => false }),
      },
    } as unknown as EditTarget;

    expect(availabilityFor(target, "")).toEqual({ cut: false, copy: true, paste: false });
    expect(selectionTextOf(target, "")).toBe("выделенное");
  });
});
