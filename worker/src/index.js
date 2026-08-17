// Serves the elvm installer at https://get.elephc.dev.
//
// text/plain rather than application/x-sh on purpose: anyone should be able to
// open the URL in a browser and read exactly what `curl | sh` would run.
// The script is identical for every client — no User-Agent negotiation, which
// would make the page unverifiable.
import script from "../../install.sh";

export default {
  fetch() {
    return new Response(script, {
      headers: {
        "content-type": "text/plain; charset=utf-8",
        "cache-control": "public, max-age=300",
      },
    });
  },
};
