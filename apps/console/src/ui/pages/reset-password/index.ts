import { define } from "@/ui/components/base";
import { AuthPage } from "../auth/shared";
export class ResetPasswordPage extends AuthPage {
  kind = "reset";
}
define("gw-page-reset-password", ResetPasswordPage);
