import type { List, ListItem } from "./types";
import { InlineHtml } from "./InlineHtml";
import { AscBlockList } from "./AscBlockList";

/**
 * Маркизированный (`ulist`) и нумерованный (`olist`) списки.
 * Вложенные списки хранятся как дочерние блоки `list_item`'а —
 * рекурсивно обходим их.
 */
export function AscList({ list }: { list: List }) {
  const ctx = list.getContext();
  const ordered = ctx === "olist";
  const Tag = ordered ? "ol" : "ul";
  const items = list.getItems();
  return (
    <Tag className={`asc-list ${ordered ? "asc-list-ordered" : "asc-list-unordered"}`}>
      {items.map((item, i) => (
        <AscListItem key={i} item={item} />
      ))}
    </Tag>
  );
}

function AscListItem({ item }: { item: ListItem }) {
  const text = item.getText();
  const blocks = item.getBlocks();
  // Дочерние блоки: либо вложенные списки, либо параграфы/иные блоки.
  const nestedLists = blocks.filter((b) => {
    const ctx = b.getContext();
    return ctx === "ulist" || ctx === "olist" || ctx === "dlist";
  });
  const otherBlocks = blocks.filter((b) => {
    const ctx = b.getContext();
    return ctx !== "ulist" && ctx !== "olist" && ctx !== "dlist";
  });
  return (
    <li className="asc-list-item">
      {text ? <InlineHtml html={text} /> : null}
      {otherBlocks.length > 0 ? <AscBlockList blocks={otherBlocks} /> : null}
      {nestedLists.length > 0 ? (
        <div className="asc-list-nested">
          {nestedLists.map((nl, i) => (
            <AscList key={i} list={nl as unknown as List} />
          ))}
        </div>
      ) : null}
    </li>
  );
}
