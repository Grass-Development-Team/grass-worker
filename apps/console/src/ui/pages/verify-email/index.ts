import { define } from "@/ui/components/base";
import { AuthPage } from "../auth/shared";
export class VerifyEmailPage extends AuthPage {
  kind = "verify";
}
define("gw-page-verify-email", VerifyEmailPage);
