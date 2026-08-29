import { describe, expect, test } from "bun:test";
import { splitSkillFrontmatter } from "../lib/skills";

describe("splitSkillFrontmatter", () => {
  test("splits the YAML block from the Markdown body", () => {
    const { frontmatter, body } = splitSkillFrontmatter(
      "---\nname: my-skill\ndescription: Does a thing.\n---\n# Title\n\nProse.\n",
    );
    expect(frontmatter).toBe("name: my-skill\ndescription: Does a thing.");
    expect(body).toBe("# Title\n\nProse.\n");
  });

  test("handles CRLF line endings and a leading BOM", () => {
    const { frontmatter, body } = splitSkillFrontmatter(
      "﻿---\r\nname: my-skill\r\n---\r\n# Title\r\n",
    );
    expect(frontmatter).toBe("name: my-skill");
    expect(body).toBe("# Title\r\n");
  });

  test("a file without frontmatter is all body", () => {
    const content = "# Just Markdown\n\n---\n\nA thematic break.\n";
    expect(splitSkillFrontmatter(content)).toEqual({ frontmatter: null, body: content });
  });

  test("stops at the first closing fence, so body `---` stays in the body", () => {
    const { frontmatter, body } = splitSkillFrontmatter(
      "---\nname: my-skill\n---\nBefore\n\n---\n\nAfter\n",
    );
    expect(frontmatter).toBe("name: my-skill");
    expect(body).toBe("Before\n\n---\n\nAfter\n");
  });
});
