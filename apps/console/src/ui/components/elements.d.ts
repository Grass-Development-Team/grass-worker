export {};
declare global {
  interface HTMLElementTagNameMap {
    "gw-tooltip": import("./tooltip").GwTooltip;
    "gw-toast": import("./toast").GwToast;
    "gw-textarea": import("./textarea").GwTextarea;
    "gw-text": import("./text").GwText;
    "gw-tabs": import("./tabs").GwTabs;
    "gw-table": import("./table").GwTable;
    "gw-spinner": import("./spinner").GwSpinner;
    "gw-skeleton": import("./skeleton").GwSkeleton;
    "gw-sidebar": import("./sidebar").GwSidebar;
    "gw-separator": import("./separator").GwSeparator;
    "gw-select": import("./select").GwSelect;
    "gw-radio-group": import("./radio-group").GwRadioGroup;
    "gw-progress": import("./progress").GwProgress;
    "gw-popover": import("./popover").GwPopover;
    "gw-dropdown-menu": import("./popover").GwDropdownMenu;
    "gw-password-input": import("./password-input").GwPasswordInput;
    "gw-pagination": import("./pagination").GwPagination;
    "gw-input-otp": import("./input-otp").GwInputOtp;
    "gw-input-group": import("./input-group").GwInputGroup;
    "gw-input": import("./input").GwInput;
    "gw-image": import("./image").GwImage;
    "gw-icon": import("./icon").GwIcon;
    "gw-field-group": import("./field-group").GwFieldGroup;
    "gw-field": import("./field").GwField;
    "gw-error-state": import("./error-state").GwErrorState;
    "gw-empty-state": import("./empty-state").GwEmptyState;
    "gw-dialog": import("./dialog").GwDialog;
    "gw-confirm-dialog": import("./dialog").GwConfirmDialog;
    "gw-sheet": import("./dialog").GwSheet;
    "gw-description-list": import("./description-list").GwDescriptionList;
    "gw-command": import("./command").GwCommand;
    "gw-combobox": import("./combobox").GwCombobox;
    "gw-checkbox": import("./checkbox").GwCheckbox;
    "gw-switch": import("./checkbox").GwSwitch;
    "gw-card": import("./card").GwCard;
    "gw-button": import("./button").GwButton;
    "gw-breadcrumb": import("./breadcrumb").GwBreadcrumb;
    "gw-badge": import("./badge").GwBadge;
    "gw-avatar": import("./avatar").GwAvatar;
    "gw-alert": import("./alert").GwAlert;
  }
}
