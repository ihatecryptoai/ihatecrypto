// What the browser's toolbar and the page panel's header both say.

import { t } from "./core.js";

/** Why a page didn't load, as the app's Rust part learned it from WebView2 (web.rs), in the Mac's words. */
export function loadError(code) {
  if (!code) return "";
  const lead = t("The page didn't load: ", "Страница не открылась: ");
  return {
    name: lead + t("there's no such site. Check the address.", "такого сайта нет. Проверь адрес."),
    timeout: lead + t("the site isn't answering. Press Reload or try later.", "сайт не отвечает. Нажми «Обновить» или попробуй позже."),
    connect: lead + t("no connection to the site. Check the internet and press Reload.", "нет связи с сайтом. Проверь интернет и нажми «Обновить»."),
    certificate: lead + t("the site's certificate isn't valid, so it isn't shown.", "у сайта недействительный сертификат, поэтому он не показан."),
    crashed: t("The page's process stopped. Press Reload.", "Процесс страницы остановился. Нажми «Обновить»."),
  }[code] || lead + t(`WebView2 error ${code.replace("other:", "")}. Press Reload.`, `ошибка WebView2 ${code.replace("other:", "")}. Нажми «Обновить».`);
}
