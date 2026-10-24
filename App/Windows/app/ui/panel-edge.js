// The panel's left edge beside its page: a drag sizes the panel, a double click gives it its own width again. It only
// reports the width (web.rs page_layout), as the headers' own edge does.

import { follow, grip } from "./panel-grip.js";

follow();
grip(null, document.getElementById("edge"));
