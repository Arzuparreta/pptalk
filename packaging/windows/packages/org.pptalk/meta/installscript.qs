function Component() {}

Component.prototype.createOperations = function() {
    component.createOperations();
    component.addOperation("CreateShortcut",
                           "@TargetDir@/pptalk-desktop.exe",
                           "@StartMenuDir@/pptalk.lnk");
    component.addOperation("Execute", "reg.exe", "add",
                           "HKCU\\Software\\Classes\\pptalk", "/ve", "/d",
                           "URL:pptalk Protocol", "/f");
    component.addOperation("Execute", "reg.exe", "add",
                           "HKCU\\Software\\Classes\\pptalk", "/v", "URL Protocol",
                           "/d", "", "/f");
    component.addOperation("Execute", "reg.exe", "add",
                           "HKCU\\Software\\Classes\\pptalk\\shell\\open\\command",
                           "/ve", "/d", "\"@TargetDir@/pptalk-desktop.exe\" \"%1\"", "/f");
    component.addOperation("Execute", "cmd.exe", "/c", "exit", "0",
                           "UNDOEXECUTE", "reg.exe", "delete",
                           "HKCU\\Software\\Classes\\pptalk", "/f");
};
