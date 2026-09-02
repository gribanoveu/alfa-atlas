import { afterEach, describe, expect, mock, test } from "bun:test";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ArtifactContent } from "../lib/artifacts";
import * as actualArtifacts from "../lib/artifacts";

// The preview goes through Rust over IPC; what is under test here is the
// draft's own behaviour, so the renderer is stubbed out.
mock.module("../lib/artifacts", () => ({
  ...actualArtifacts,
  artifactRender: async () => ({ kind: "jiraTicket", wiki: "" }),
}));

const { JiraTicketBuilder, sectionNumbers } = await import(
  "../components/Artifacts/JiraTicketBuilder"
);

afterEach(cleanup);

type Ticket = Extract<ArtifactContent, { kind: "jiraTicket" }>;

function ticket(fields: Partial<Ticket> = {}): Ticket {
  return {
    kind: "jiraTicket",
    issueKey: "",
    why: "",
    outcome: "",
    inScope: [],
    outOfScope: [],
    solution: "",
    acceptanceCriteria: [],
    definitionOfDone: [],
    risks: [],
    links: [],
    ...fields,
  };
}

function renderBuilder(spec = ticket()) {
  const changes: ArtifactContent[] = [];
  render(<JiraTicketBuilder spec={spec} onChange={(next) => changes.push(next)} />);
  return changes;
}

describe("JiraTicketBuilder", () => {
  // The point of the redesign: a dozen acceptance criteria used to be a
  // dozen inputs and a click before every entry.
  test("a list section is one field, one line per item", () => {
    const changes = renderBuilder();
    const field = screen.getByLabelText("Критерии приемки (AC)");

    fireEvent.change(field, { target: { value: "Первый\nВторой" } });

    expect(changes.at(-1)).toMatchObject({ acceptanceCriteria: ["Первый", "Второй"] });
  });

  test("an existing list shows up as lines", () => {
    renderBuilder(ticket({ inScope: ["Кнопка", "Экран"] }));
    expect((screen.getByLabelText("Что входит в задачу") as HTMLTextAreaElement).value).toBe(
      "Кнопка\nЭкран",
    );
  });

  // A trailing blank line is what someone pressing Enter is in the middle
  // of typing — stripping it would fight the cursor. The renderer drops
  // blanks when it builds the ticket.
  test("a blank line survives editing", () => {
    const changes = renderBuilder(ticket({ risks: ["Округление"] }));

    fireEvent.change(screen.getByLabelText("Риски"), {
      target: { value: "Округление\n" },
    });

    expect(changes.at(-1)).toMatchObject({ risks: ["Округление", ""] });
  });

  test("prose sections stay plain strings", () => {
    const changes = renderBuilder();

    fireEvent.change(screen.getByLabelText("Почему задача существует"), {
      target: { value: "Нет выгрузки" },
    });

    expect(changes.at(-1)).toMatchObject({ why: "Нет выгрузки" });
  });

  test("an empty section is marked as one that will not reach Jira", () => {
    renderBuilder(ticket({ why: "Проблема" }));
    // Seven of the eight text sections; links are folded away, so that
    // heading is not on screen to be marked.
    expect(screen.getAllByText("не попадёт в задачу").length).toBe(7);
  });

  test("links stay folded until they hold something", () => {
    renderBuilder();
    expect(screen.queryByText("Добавить ссылку")).toBeNull();

    fireEvent.click(screen.getByText("Ссылки"));
    expect(screen.getByText("Добавить ссылку")).toBeDefined();
  });

  test("a ticket that already has links opens with them visible", () => {
    renderBuilder(ticket({ links: [{ kind: "GIT", url: "https://git.example.net/x", title: "" }] }));
    expect(screen.getByLabelText("URL ссылки 1")).toBeDefined();
  });
});

/** Mirrors `domain::artifact_render`: numbers are assigned after the empty
 *  sections are dropped, and the draft shows that live so nobody discovers
 *  the renumbering only after pasting into Jira. */
describe("sectionNumbers", () => {
  test("numbers only the filled sections, in order", () => {
    const numbers = sectionNumbers(
      ticket({
        why: "Проблема",
        acceptanceCriteria: ["Пользователь видит X"],
        links: [{ kind: "GIT", url: "https://git.example.net/x", title: "" }],
      }),
    );

    expect(numbers.why).toBe(1);
    expect(numbers.acceptanceCriteria).toBe(2);
    expect(numbers.links).toBe(3);
    expect(numbers.outcome).toBeNull();
    expect(numbers.risks).toBeNull();
  });

  test("blank entries do not make a section count as filled", () => {
    const numbers = sectionNumbers(
      ticket({
        why: "Проблема",
        inScope: ["  ", ""],
        links: [{ kind: "GIT", url: "  ", title: "" }],
      }),
    );

    expect(numbers.why).toBe(1);
    expect(numbers.inScope).toBeNull();
    expect(numbers.links).toBeNull();
  });

  test("an empty ticket numbers nothing", () => {
    const numbers = sectionNumbers(ticket());
    expect(Object.values(numbers).every((n) => n === null)).toBe(true);
  });
});
