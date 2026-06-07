"""Web fetching for AI agent frameworks via servo-fetch (shown with Strands Agents)."""

from strands import Agent, tool

import servo_fetch


@tool
def fetch_page(url: str) -> str:
    """Fetch one web page and return its main content as clean Markdown."""
    return servo_fetch.fetch(url).markdown


@tool
def map_site(url: str, limit: int = 50) -> list[str]:
    """Discover URLs on a website from its sitemap, without rendering pages."""
    return [entry.url for entry in servo_fetch.Client().map(url, limit=limit)]


if __name__ == "__main__":
    agent = Agent(tools=[fetch_page, map_site])
    agent("Fetch https://example.com and summarize what the site is for in one sentence.")
