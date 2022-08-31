::parser_transition{
    print(copyinstr(arg0));
}

::parser_dropped {
    printf("parser dropped\n");
}

::parser_accepted {
    printf("parser accepted\n");
}

::control_apply{
    print(copyinstr(arg0));
}

::control_dropped {
    printf("control dropped\n");
}

::control_accepted {
    printf("control accepted\n");
}

::control_table_hit {
    print(copyinstr(arg0));
}

::control_table_miss {
    print(copyinstr(arg0));
}
