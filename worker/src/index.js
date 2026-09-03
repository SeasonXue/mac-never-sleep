import { Board, handleApi } from "./board.js";

/**
 * Per-home live state. One DO is enough: a handful of Macs heartbeating every
 * few seconds, phones polling the same object.
 */
export class BoardHub {
  constructor(ctx) {
    this.ctx = ctx;
  }

  async fetch(request) {
    const stored = (await this.ctx.storage.get("board")) || null;
    const board = Board.fromJSON(stored);
    board.nowSecs = () => Math.floor(Date.now() / 1000);
    const response = await handleApi(board, request);
    await this.ctx.storage.put("board", board.toJSON());
    return response;
  }
}

export default {
  /**
   * @param {Request} request
   * @param {{ BOARD: DurableObjectNamespace, ASSETS?: { fetch: typeof fetch } }} env
   */
  async fetch(request, env) {
    const url = new URL(request.url);
    if (url.pathname === "/api" || url.pathname.startsWith("/api/")) {
      const id = env.BOARD.idFromName("board");
      return env.BOARD.get(id).fetch(request);
    }
    if (env.ASSETS) {
      return env.ASSETS.fetch(request);
    }
    return new Response("Not found", { status: 404 });
  },
};
