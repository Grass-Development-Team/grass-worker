import { define } from "@/ui/components/base";
import { AuthPage } from "../auth/shared";
export class ForgotPasswordPage extends AuthPage {
  kind = "forgot";
}
define("gw-page-forgot-password", ForgotPasswordPage);
