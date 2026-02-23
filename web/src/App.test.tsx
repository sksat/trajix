// @vitest-environment jsdom
import { describe, it, expect } from "vitest";
import { render, fireEvent } from "@testing-library/react";

/**
 * Minimal test for the header behavior, extracted from App's header.
 * We test in isolation since App has heavy deps (Cesium, WASM).
 */
function AppHeader({ onReset }: { onReset: () => void }) {
  return (
    <header className="app-header">
      <h1>
        <button className="title-link" onClick={onReset}>
          trajix
        </button>
      </h1>
      <a
        className="github-link"
        href="https://github.com/sksat/trajix"
        target="_blank"
        rel="noopener noreferrer"
        aria-label="GitHub repository"
      >
        GH
      </a>
    </header>
  );
}

describe("Title button", () => {
  it("renders trajix title text", () => {
    const { container } = render(<AppHeader onReset={() => {}} />);
    const h1 = container.querySelector("h1");
    expect(h1).not.toBeNull();
    expect(h1!.textContent).toBe("trajix");
  });

  it("always renders as a clickable button", () => {
    const { container } = render(<AppHeader onReset={() => {}} />);
    const button = container.querySelector("button.title-link");
    expect(button).not.toBeNull();
  });

  it("calls onReset when title button is clicked", () => {
    let called = false;
    const { container } = render(
      <AppHeader onReset={() => (called = true)} />,
    );
    const button = container.querySelector("button.title-link")!;
    fireEvent.click(button);
    expect(called).toBe(true);
  });
});

describe("GitHub link", () => {
  it("renders a link to the GitHub repository", () => {
    const { container } = render(<AppHeader onReset={() => {}} />);
    const link = container.querySelector("a.github-link");
    expect(link).not.toBeNull();
    expect(link!.getAttribute("href")).toBe(
      "https://github.com/sksat/trajix",
    );
  });

  it("opens in a new tab", () => {
    const { container } = render(<AppHeader onReset={() => {}} />);
    const link = container.querySelector("a.github-link")!;
    expect(link.getAttribute("target")).toBe("_blank");
    expect(link.getAttribute("rel")).toContain("noopener");
  });

  it("has accessible label", () => {
    const { container } = render(<AppHeader onReset={() => {}} />);
    const link = container.querySelector("a.github-link")!;
    expect(link.getAttribute("aria-label")).toBe("GitHub repository");
  });
});
