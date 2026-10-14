import { unsafeCSS } from "lit";
import { define } from "../base";
import { GwInput } from "../input";
import styles from "./style.css?inline";
export class GwInputOtp extends GwInput {
  static styles = [...GwInput.styles, unsafeCSS(styles)];
  constructor() {
    super();
    this.autocomplete = "one-time-code";
    this.inputmode = "numeric";
    this.pattern = "[0-9]{6}";
    this.minLength = 6;
    this.maxLength = 6;
    this.label = "Verification code";
  }
}
define("gw-input-otp", GwInputOtp);
