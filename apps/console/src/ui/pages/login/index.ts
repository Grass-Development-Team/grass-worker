import { define } from "@/ui/components/base";
import { AuthPage } from "../auth/shared";
export class LoginPage extends AuthPage {
  kind = "login";
}
define("gw-page-login", LoginPage);
