import i18n from "i18next";
import { initReactI18next } from "react-i18next";

import bn from "./locales/bn";
import en from "./locales/en";

void i18n.use(initReactI18next).init({
  resources: {
    en: { translation: en },
    bn: { translation: bn },
  },
  lng: "en",
  fallbackLng: "en",
  interpolation: { escapeValue: false },
  returnNull: false,
});

export default i18n;
