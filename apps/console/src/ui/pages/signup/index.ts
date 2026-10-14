import { define } from "@/ui/components/base";
import { AuthPage } from "../auth/shared";
export class SignupPage extends AuthPage {
  kind = "signup";
}
define("gw-page-signup", SignupPage);
